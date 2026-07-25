//! `POST /api/v1/runs/registrations` — register an external run.
//!
//! The /runs pane reflects reality only if every execution that wants to
//! show up there registers here. The three sources that need to surface
//! are distinct at every level of the stack:
//!
//! 1. **Live forkd gate execution** — `RunOriginKind::Gate` + a
//!    [`RunGateOrigin`] payload. Carries endpoint, sandbox_id, model.
//! 2. **Live hermetic SessionEnd referee score** — `RunOriginKind::Referee` + a
//!    [`RunRefereeOrigin`] payload. Carries tier, base_sha, head_sha,
//!    dispatch_path.
//! 3. **Backfill retro-score** — `RunOriginKind::RefereeBackfill` + a
//!    [`RunRefereeBackfillOrigin`] payload. Same fields as referee PLUS
//!    `backfill_at` and `requester`. The presence of `backfill_at` is the
//!    single discriminator between a live referee score and a backfill row at
//!    the data layer.
//!
//! The handler refuses to coerce any of these into another:
//! - `api` is rejected outright (it has its own `POST /runs` manifest path with
//!   full Graphviz workflow).
//! - A null / missing verdict is rejected (200 with a `Pass` is the failure
//!   mode this plane exists to prevent).
//! - A non-SHA `base_ref` is rejected for referee variants.
//! - A backfill row missing `backfill_at` is rejected.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use fabro_api::types::RepositoryRef;
use fabro_store::Error as StoreError;
use fabro_types::run_origin::{RefereeVerdict, RunOriginDetails};
use fabro_types::run_summary::RunOriginKind;
use fabro_types::timing::RunTiming;
use fabro_types::{
    FailureDetail, FailureReason, Principal, RunClientProvenance, RunId, RunOrigin, RunProvenance,
    RunServerProvenance, SuccessReason,
};
use fabro_util::version::FABRO_VERSION;
use fabro_workflow::event::{Event, append_event};
use serde::Deserialize;
use tracing::info;
use ulid::Ulid;

use super::super::AppState;
use crate::error::ApiError;
use crate::principal_middleware::RequiredUser;

/// Request body for `POST /api/v1/runs/registrations`.
///
/// OpenAPI-first: the schema is generated from `RunRegistrationRequest`
/// in `docs/public/api-reference/fabro-api.yaml`. Any new field here MUST
/// be added to the YAML first so the progenitor types stay in sync.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RegisterExternalRunRequest {
    /// Caller-supplied ULID. Re-registration of the same `run_id` returns
    /// the existing run (idempotent), not a 409.
    pub run_id:     Option<String>,
    pub origin:     RunOriginDetails,
    #[serde(default)]
    pub title:      Option<String>,
    #[serde(default)]
    pub goal:       Option<String>,
    #[serde(default)]
    pub summary:    Option<String>,
    #[serde(default)]
    pub repository: Option<RepositoryRef>,
}

/// Build the [`RunOriginKind`] from a request, validating the kind against
/// the `RunOriginDetails` variant. Mismatches (`gate` payload inside a
/// `referee` envelope) are rejected before any database work happens.
fn validate_origin_match(kind: RunOriginKind, details: &RunOriginDetails) -> Result<(), ApiError> {
    if details.kind() != kind {
        return Err(ApiError::bad_request(format!(
            "origin.kind={kind:?} does not match details.kind={:?}",
            details.kind()
        )));
    }
    Ok(())
}

/// `POST /api/v1/runs/registrations` — register an external (gate /
/// referee / backfill) run as a terminal-state sentinel row.
pub(crate) async fn register_external_run(
    RequiredUser(actor): RequiredUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterExternalRunRequest>,
) -> Response {
    match Box::pin(register_external_run_inner(
        state,
        req,
        Principal::User(actor),
    ))
    .await
    {
        Ok((run_json, status)) => (status, Json(run_json)).into_response(),
        Err(api_err) => api_err.into_response(),
    }
}

async fn register_external_run_inner(
    state: Arc<AppState>,
    req: RegisterExternalRunRequest,
    actor: Principal,
) -> Result<(serde_json::Value, StatusCode), ApiError> {
    // -- 1. Validate the origin matches its details payload -----------------
    let kind = match &req.origin {
        RunOriginDetails::Gate(_) => RunOriginKind::Gate,
        RunOriginDetails::Referee(_) => RunOriginKind::Referee,
        RunOriginDetails::RefereeBackfill(_) => RunOriginKind::RefereeBackfill,
    };
    if matches!(kind, RunOriginKind::Api) {
        return Err(ApiError::bad_request(
            "api origin is registered via POST /api/v1/runs, not /registrations",
        ));
    }
    validate_origin_match(kind, &req.origin)?;

    // -- 2. Validate payload invariants -------------------------------------
    let verdict = extract_verdict(&req.origin);
    if let RunOriginDetails::Referee(origin) = &req.origin {
        validate_sha("base_sha", &origin.base_sha)?;
        validate_sha("head_sha", &origin.head_sha)?;
    }
    if let RunOriginDetails::RefereeBackfill(origin) = &req.origin {
        validate_sha("base_sha", &origin.base_sha)?;
        validate_sha("head_sha", &origin.head_sha)?;
        // `backfill_at` and `requester` are required by the schema, so a
        // missing field would have failed deserialization. We re-assert
        // here as a defense-in-depth check.
        if origin.requester.trim().is_empty() {
            return Err(ApiError::bad_request(
                "referee_backfill origin requires non-empty `requester`",
            ));
        }
    }

    // -- 3. Resolve / generate the run_id -----------------------------------
    let run_id: RunId = match req.run_id.as_deref() {
        Some(raw) => raw
            .parse::<RunId>()
            .map_err(|err| ApiError::bad_request(format!("invalid run_id `{raw}`: {err}")))?,
        None => RunId::from(Ulid::new()),
    };

    // -- 4. Idempotent: if the run already exists, return it unchanged. -----
    if let Some(existing) = state
        .stores
        .runs
        .get_cached_summary(&run_id, Utc::now())
        .await
        .map_err(|err| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
    {
        // Re-registration is only allowed when the existing run carries the
        // same origin kind. Mismatched kind is a real error: two systems
        // cannot both claim a row.
        if existing.origin.kind != kind {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!(
                    "run {run_id} already exists with origin.kind={:?}; refusing to re-register as {kind:?}",
                    existing.origin.kind
                ),
            ));
        }
        let summary = state.decorate_run_summary(existing).await;
        return Ok((
            serde_json::to_value(&summary).map_err(into_internal)?,
            StatusCode::OK,
        ));
    }

    // -- 5. Create the RunDatabase entry ------------------------------------
    let run_store = match state.stores.runs.create_run(&run_id).await {
        Ok(s) => s,
        Err(StoreError::RunAlreadyExists(_)) => {
            // Race: another caller won. Re-fetch and return idempotent.
            let summary = state
                .stores
                .runs
                .get_cached_summary(&run_id, Utc::now())
                .await
                .map_err(|err| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
                .ok_or_else(|| ApiError::not_found("Run not found after race."))?;
            let summary = state.decorate_run_summary(summary).await;
            return Ok((
                serde_json::to_value(&summary).map_err(into_internal)?,
                StatusCode::OK,
            ));
        }
        Err(err) => {
            return Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                err.to_string(),
            ));
        }
    };

    // -- 6. Compose provenance ---------------------------------------------
    let provenance = build_provenance(&actor, &req.origin);

    // -- 7. Emit RunCreated -------------------------------------------------
    let title = req
        .title
        .clone()
        .unwrap_or_else(|| infer_external_title(&req.origin));
    let goal = req
        .goal
        .clone()
        .unwrap_or_else(|| "external gate / referee score".to_string());
    let workflow_source = Some("digraph external {}".to_string());
    let mut label_pairs = build_labels(&req.origin);
    if let Some(summary) = req
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        label_pairs.push(("source.summary".to_string(), summary.to_string()));
    }
    if let Some(repository) = &req.repository {
        label_pairs.push(("repo.name".to_string(), repository.name.clone()));
        if let Some(origin_url) = &repository.origin_url {
            label_pairs.push(("repo.origin_url".to_string(), origin_url.clone()));
        }
        let provider = match repository.provider {
            fabro_types::RepositoryProvider::Github => "github",
            fabro_types::RepositoryProvider::Git => "git",
            fabro_types::RepositoryProvider::Unknown => "unknown",
        };
        label_pairs.push(("repo.provider".to_string(), provider.to_string()));
    }
    let labels: std::collections::BTreeMap<String, String> = label_pairs.into_iter().collect();
    let mut graph = fabro_types::Graph::new("external");
    graph
        .attrs
        .insert("goal".to_string(), fabro_types::AttrValue::String(goal));
    let run_dir = format!("external/{run_id}");
    let source_directory = None;
    let workflow_slug = Some("external".to_string());
    let run_origin = RunOrigin {
        kind,
        details: Some(Box::new(req.origin.clone())),
    };

    let now = Utc::now();
    let run_created = Event::RunCreated {
        run_id,
        title: Some(title.clone()),
        settings: serde_json::to_value(fabro_types::WorkflowSettings::default())
            .map_err(into_internal)?,
        graph: serde_json::to_value(&graph).map_err(into_internal)?,
        workflow_source: workflow_source.clone(),
        workflow_config: None,
        labels: labels.clone(),
        run_dir: run_dir.clone(),
        source_directory: source_directory.clone(),
        workflow_slug: workflow_slug.clone(),
        automation: None,
        db_prefix: None,
        provenance: provenance.clone(),
        origin: Some(run_origin),
        manifest_blob: None,
        git: None,
        fork_source_ref: None,
        retried_from: None,
        parent_id: None,
        web_url: state.run_web_url(&run_id),
    };
    append_event(&run_store, &run_id, &run_created)
        .await
        .map_err(into_internal)?;

    // -- 7b. Drive the run through the lifecycle to a running state ---------
    // The status machine only allows terminal transitions out of `Running`
    // (and non-cancel `Failed` only from Starting/Running/Blocked/Paused),
    // so a sentinel run must replay Submitted -> Runnable -> Starting ->
    // Running before the terminal event, or the projection rejects it.
    for lifecycle_event in [
        Event::RunRunnable {
            source: fabro_types::RunRunnableSource::StartRequested,
            actor:  None,
        },
        Event::RunStarting,
        Event::RunRunning,
    ] {
        append_event(&run_store, &run_id, &lifecycle_event)
            .await
            .map_err(into_internal)?;
    }

    // -- 8. Emit the terminal event ----------------------------------------
    let (terminal_event, run_status_kind) = match verdict {
        RefereeVerdict::Pass => (
            Event::WorkflowRunCompleted {
                timing:               RunTiming::wall_only(0),
                artifact_count:       0,
                status:               "succeeded".to_string(),
                reason:               SuccessReason::Completed,
                total_usd_micros:     None,
                final_git_commit_sha: None,
                final_patch:          None,
                diff_summary:         None,
                billing:              None,
            },
            "succeeded",
        ),
        RefereeVerdict::Fail => (
            Event::WorkflowRunFailed {
                failure:              fabro_types::RunFailure {
                    reason: FailureReason::WorkflowError,
                    detail: FailureDetail {
                        message:          build_failure_detail(&req.origin),
                        causes:           Vec::new(),
                        category:         fabro_types::FailureCategory::Deterministic,
                        system_actor:     None,
                        signature:        Some(fabro_types::FailureSignature(
                            "external.gate.fail".to_string(),
                        )),
                        exec_output_tail: None,
                    },
                },
                timing:               RunTiming::wall_only(0),
                final_git_commit_sha: None,
                final_patch:          None,
                diff_summary:         None,
                billing:              None,
            },
            "failed",
        ),
        RefereeVerdict::Inconclusive => (
            Event::WorkflowRunFailed {
                failure:              fabro_types::RunFailure {
                    reason: FailureReason::TransientInfra,
                    detail: FailureDetail {
                        message:          build_failure_detail(&req.origin),
                        causes:           Vec::new(),
                        category:         fabro_types::FailureCategory::TransientInfra,
                        system_actor:     None,
                        signature:        Some(fabro_types::FailureSignature(
                            "external.gate.inconclusive".to_string(),
                        )),
                        exec_output_tail: None,
                    },
                },
                timing:               RunTiming::wall_only(0),
                final_git_commit_sha: None,
                final_patch:          None,
                diff_summary:         None,
                billing:              None,
            },
            "failed",
        ),
    };
    append_event(&run_store, &run_id, &terminal_event)
        .await
        .map_err(into_internal)?;

    info!(
        run_id = %run_id,
        origin = %kind,
        verdict = ?verdict,
        status = run_status_kind,
        title = %title,
        "External run registered"
    );

    // -- 9. Return the decorated summary -----------------------------------
    let summary = state
        .stores
        .runs
        .get_cached_summary(&run_id, now)
        .await
        .map_err(|err| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| ApiError::not_found("Run not found after registration."))?;
    let summary = state.decorate_run_summary(summary).await;
    let body = serde_json::to_value(&summary).map_err(into_internal)?;
    Ok((body, StatusCode::CREATED))
}

fn extract_verdict(origin: &RunOriginDetails) -> RefereeVerdict {
    match origin {
        RunOriginDetails::Gate(g) => g.verdict,
        RunOriginDetails::Referee(r) => r.verdict,
        RunOriginDetails::RefereeBackfill(r) => r.verdict,
    }
}

fn validate_sha(field: &'static str, value: &str) -> Result<(), ApiError> {
    let bytes = value.as_bytes();
    if bytes.len() != 40 || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return Err(ApiError::bad_request(format!(
            "{field} must be a 40-char hex SHA, got `{value}`"
        )));
    }
    Ok(())
}

fn build_provenance(actor: &Principal, origin: &RunOriginDetails) -> RunProvenance {
    let user_agent = match origin {
        RunOriginDetails::Gate(g) => format!("forkd-gate:{}", g.endpoint),
        RunOriginDetails::Referee(r) => format!("referee-dispatch:{}:{}", r.dispatch_path, r.tier),
        RunOriginDetails::RefereeBackfill(r) => {
            format!("backfill-referee:{}:{}", r.requester, r.tier)
        }
    };
    let client = RunClientProvenance {
        user_agent: Some(user_agent),
        name:       Some("fabro-registrations".to_string()),
        version:    Some(env!("CARGO_PKG_VERSION").to_string()),
    };
    RunProvenance {
        server:  Some(RunServerProvenance {
            version: FABRO_VERSION.to_string(),
        }),
        client:  Some(client),
        subject: actor.clone(),
    }
}

fn build_labels(origin: &RunOriginDetails) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    out.push(("origin.kind".to_string(), origin.kind().to_string()));
    match origin {
        RunOriginDetails::Gate(g) => {
            out.push(("source.endpoint".to_string(), g.endpoint.clone()));
            out.push(("source.model".to_string(), g.model.clone()));
            out.push(("source.sandbox_id".to_string(), g.sandbox_id.clone()));
        }
        RunOriginDetails::Referee(r) => {
            out.push(("source.tier".to_string(), r.tier.clone()));
            out.push((
                "source.dispatch_path".to_string(),
                r.dispatch_path.to_string(),
            ));
            if r.synthetic {
                out.push(("source.synthetic".to_string(), "true".to_string()));
            }
            if let Some(branch) = &r.branch {
                out.push(("source.branch".to_string(), branch.clone()));
            }
        }
        RunOriginDetails::RefereeBackfill(r) => {
            out.push(("source.tier".to_string(), r.tier.clone()));
            out.push(("source.requester".to_string(), r.requester.clone()));
            out.push((
                "source.dispatch_path".to_string(),
                r.dispatch_path.to_string(),
            ));
            if r.synthetic {
                out.push(("source.synthetic".to_string(), "true".to_string()));
            }
        }
    }
    out
}

fn infer_external_title(origin: &RunOriginDetails) -> String {
    match origin {
        RunOriginDetails::Gate(g) => format!("forkd gate: {} @ {}", g.model, g.sandbox_id),
        RunOriginDetails::Referee(r) => format!("referee: {} ({})", r.tier, r.dispatch_path),
        RunOriginDetails::RefereeBackfill(r) => {
            format!("referee backfill: {} ({})", r.tier, r.requester)
        }
    }
}

fn build_failure_detail(origin: &RunOriginDetails) -> String {
    let source = match origin {
        RunOriginDetails::Gate(g) => format!("gate@{} ({})", g.endpoint, g.sandbox_id),
        RunOriginDetails::Referee(r) => format!("referee/{} via {}", r.tier, r.dispatch_path),
        RunOriginDetails::RefereeBackfill(r) => {
            format!("referee_backfill/{} via {}", r.tier, r.requester)
        }
    };
    match origin {
        RunOriginDetails::Gate(g) => g
            .error
            .clone()
            .unwrap_or_else(|| format!("{source} returned a fail verdict")),
        RunOriginDetails::Referee(r) => r
            .error
            .clone()
            .unwrap_or_else(|| format!("{source} returned a fail verdict")),
        RunOriginDetails::RefereeBackfill(r) => r
            .error
            .clone()
            .unwrap_or_else(|| format!("{source} returned a fail verdict")),
    }
}

fn into_internal<E: std::fmt::Display>(err: E) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

// =============================================================================
// Tests — the provenance-distinguishability contract
// =============================================================================
//
// These tests exist to fail if a future refactor collapses the three
// provenance sources into a single undifferentiated type. The contract is
// encoded in the test names so a future reader who sees a failure
// immediately understands what was protected.

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use chrono::Utc;
    use fabro_types::run_origin::{
        RefereeDispatchPath, RefereeVerdict, RunGateOrigin, RunRefereeBackfillOrigin,
        RunRefereeOrigin,
    };
    use tower::ServiceExt;

    use super::*;
    use crate::test_support::{build_test_router, test_app_state};

    fn gate_origin(verdict: RefereeVerdict) -> RunOriginDetails {
        RunOriginDetails::Gate(RunGateOrigin {
            endpoint: "http://dellsrv:8891".to_string(),
            sandbox_id: "sb-test".to_string(),
            model: "claude-sonnet-5[1m]".to_string(),
            verdict,
            verdict_at: Utc::now(),
            score: None,
            valset_hash: None,
            gate_log: None,
            error: None,
        })
    }

    fn referee_origin(verdict: RefereeVerdict) -> RunOriginDetails {
        RunOriginDetails::Referee(RunRefereeOrigin {
            tier: "minimax".to_string(),
            base_sha: "a".repeat(40),
            head_sha: "b".repeat(40),
            verdict,
            verdict_at: Utc::now(),
            dispatch_path: RefereeDispatchPath::AutoDetect,
            branch: Some("T-test-mm".to_string()),
            synthetic: false,
            score: None,
            valset_hash: None,
            error: None,
        })
    }

    fn backfill_origin(verdict: RefereeVerdict) -> RunOriginDetails {
        RunOriginDetails::RefereeBackfill(RunRefereeBackfillOrigin {
            tier: "sonnet".to_string(),
            base_sha: "c".repeat(40),
            head_sha: "d".repeat(40),
            verdict,
            verdict_at: Utc::now(),
            dispatch_path: RefereeDispatchPath::RefereeBackfill,
            branch: Some("T-backfill-sn".to_string()),
            synthetic: false,
            score: None,
            valset_hash: None,
            error: None,
            backfill_at: Utc::now(),
            requester: "backfill-referee.sh".to_string(),
        })
    }

    fn post_registration(body: serde_json::Value, run_id: &str) -> Request<Body> {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "run_id".to_string(),
            serde_json::Value::String(run_id.to_string()),
        );
        payload.insert("origin".to_string(), body);
        Request::builder()
            .method("POST")
            .uri("/api/v1/runs/registrations")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::Value::Object(payload)).unwrap(),
            ))
            .unwrap()
    }

    async fn body_value(resp: Response) -> serde_json::Value {
        use axum::body::to_bytes;
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        // Rejection paths (axum's JSON extractor returning 422 on a
        // deserialize error) carry a plain-text body, not JSON. Callers that
        // only inspect the status still call this, so fall back to Null
        // rather than panicking on a non-JSON body.
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    fn sample_run_id(_label: &str) -> String {
        // A fresh ULID per call. Must be a valid 26-char Crockford base32
        // ULID because the handler parses it with `RunId::from_str`; a
        // hand-built label string would be rejected with 400. `Ulid::new()`
        // guarantees uniqueness so distinct calls never collide.
        RunId::new().to_string()
    }

    #[tokio::test]
    async fn registration_requires_stringent_origin_kind() {
        // POST with an `api` origin kind is rejected — that path has its
        // own `POST /api/v1/runs` manifest endpoint.
        let state = test_app_state();
        let app = build_test_router(state);

        // Build a fake "api" envelope by abusing the oneOf: post a referee
        // payload but rename the variant in the JSON to "api". The
        // discriminator is enforced by serde on the inbound `RunOriginDetails`,
        // so this should fail at the deserialize step.
        let body = serde_json::json!({
            "kind": "api",
            "tier": "minimax",
            "base_sha": "a".repeat(40),
            "head_sha": "b".repeat(40),
            "verdict": "pass",
            "verdict_at": Utc::now(),
            "dispatch_path": "auto_detect",
        });
        let response = app
            .oneshot(post_registration(body, &sample_run_id("api-reject")))
            .await
            .unwrap();
        let status = response.status();
        let body = body_value(response).await;
        // An unknown `kind` fails serde's tagged-enum deserialization, which
        // axum's JSON extractor surfaces as 422; the explicit `api`-kind
        // guard in the handler is defense-in-depth behind that. Either
        // rejection status satisfies the "api is not registrable here"
        // contract.
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "api origin must be rejected; got {status}, body={body}"
        );
    }

    #[tokio::test]
    async fn registration_accepts_each_non_api_origin_kind() {
        // The headline test for the three-source contract: each non-`api`
        // kind is accepted with 201 and the kind is preserved verbatim.
        // If a future refactor collapses the three kinds into a single
        // variant, this test fails because the inbound discriminator
        // stops being meaningful.
        for (label, origin) in [
            ("gate", gate_origin(RefereeVerdict::Pass)),
            ("referee", referee_origin(RefereeVerdict::Pass)),
            ("referee_backfill", backfill_origin(RefereeVerdict::Fail)),
        ] {
            let state = test_app_state();
            let app = build_test_router(state);
            let body = serde_json::to_value(&origin).unwrap();
            let run_id = sample_run_id(label);
            let response = app.oneshot(post_registration(body, &run_id)).await.unwrap();
            let status = response.status();
            let body = body_value(response).await;
            assert_eq!(status, StatusCode::CREATED, "{label}: body={body}");
            // The kind MUST come back as the kind we sent. This is the
            // assertion that protects the property most worth keeping.
            assert_eq!(
                body["origin"]["kind"].as_str(),
                Some(label),
                "{label}: round-tripped kind must match"
            );
        }
    }

    #[tokio::test]
    async fn registration_idempotent_on_run_id() {
        // Re-registering the same run_id with the same kind returns 200
        // and the existing run, not 409 and not a duplicate.
        let state = test_app_state();
        let app = build_test_router(state);
        let run_id = sample_run_id("idem");
        let body = serde_json::to_value(gate_origin(RefereeVerdict::Pass)).unwrap();

        let first = app
            .clone()
            .oneshot(post_registration(body.clone(), &run_id))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED, "first should 201");

        let second = app
            .clone()
            .oneshot(post_registration(body, &run_id))
            .await
            .unwrap();
        assert_eq!(
            second.status(),
            StatusCode::OK,
            "second should be idempotent 200"
        );
    }

    #[tokio::test]
    async fn registration_rejects_mismatched_origin_kind() {
        // A `gate` payload inside a `referee` envelope (or any mismatch)
        // is rejected before any database work happens.
        let state = test_app_state();
        let app = build_test_router(state);
        // Post a gate payload with its `kind` tag renamed to "referee" to
        // force a discriminator mismatch at the boundary.
        let gate_payload = serde_json::to_value(gate_origin(RefereeVerdict::Pass)).unwrap();
        let mut mismatched = gate_payload.clone();
        mismatched["kind"] = serde_json::Value::String("referee".to_string());
        // Missing base_sha — should 400 from validation, not from kind
        // mismatch. To test mismatch alone we'd need to populate base_sha
        // in the gate struct, but the gate struct has no base_sha field;
        // serde's deserialization will fail. That's fine — the test
        // asserts that an unauthentic gate-as-referee POST is rejected.
        let response = app
            .oneshot(post_registration(mismatched, &sample_run_id("mismatch")))
            .await
            .unwrap();
        let status = response.status();
        let _ = body_value(response).await;
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "mismatched kind must be rejected; got {status}"
        );
    }

    #[tokio::test]
    async fn registration_rejects_non_sha_base_ref() {
        // A `referee` origin with `base_sha: "main"` is rejected. The
        // field is documented as a 40-char hex SHA — ref names like
        // "main" must be refused at the boundary, not silently accepted.
        let state = test_app_state();
        let app = build_test_router(state);
        let mut origin = referee_origin(RefereeVerdict::Pass);
        if let RunOriginDetails::Referee(r) = &mut origin {
            r.base_sha = "main".to_string();
        }
        let body = serde_json::to_value(&origin).unwrap();
        let response = app
            .oneshot(post_registration(body, &sample_run_id("non-sha")))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "non-SHA base_sha must be rejected"
        );
    }

    #[tokio::test]
    async fn registration_rejects_non_sha_head_ref() {
        // Same guard for `head_sha`. The pair is symmetric; if
        // `head_sha` were a ref name, downstream GEPA join keys would
        // be ambiguous.
        let state = test_app_state();
        let app = build_test_router(state);
        let mut origin = referee_origin(RefereeVerdict::Pass);
        if let RunOriginDetails::Referee(r) = &mut origin {
            r.head_sha = "HEAD".to_string();
        }
        let body = serde_json::to_value(&origin).unwrap();
        let response = app
            .oneshot(post_registration(body, &sample_run_id("non-sha-head")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn backfill_requires_backfill_at_field() {
        // A backfill payload without `backfill_at` cannot deserialize
        // because the field is required. The test asserts the 400.
        let state = test_app_state();
        let app = build_test_router(state);
        let body = serde_json::json!({
            "kind": "referee_backfill",
            "tier": "sonnet",
            "base_sha": "c".repeat(40),
            "head_sha": "d".repeat(40),
            "verdict": "pass",
            "verdict_at": Utc::now(),
            "dispatch_path": "referee_backfill",
            "requester": "backfill-referee.sh",
            // no backfill_at
        });
        let response = app
            .oneshot(post_registration(body, &sample_run_id("no-backfill-at")))
            .await
            .unwrap();
        let status = response.status();
        let _ = body_value(response).await;
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "backfill without backfill_at must be rejected; got {status}"
        );
    }

    #[tokio::test]
    async fn runs_list_distinguishes_three_sources() {
        // The single most important test for the property the user
        // explicitly called out: register one run per kind, then
        // `GET /runs` returns three rows whose `origin.kind` are
        // pairwise distinct and whose `backfill_at` is present only
        // on the backfill row. If a future refactor maps all three to
        // a single undifferentiated type, this test fails.
        let state = test_app_state();
        let app = build_test_router(state);

        // 1) Register one of each kind.
        for (label, origin) in [
            ("gate", gate_origin(RefereeVerdict::Pass)),
            ("referee", referee_origin(RefereeVerdict::Pass)),
            ("referee_backfill", backfill_origin(RefereeVerdict::Fail)),
        ] {
            let body = serde_json::to_value(&origin).unwrap();
            let response = app
                .clone()
                .oneshot(post_registration(body, &sample_run_id(label)))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::CREATED,
                "{label}: should register"
            );
        }

        // 2) List and assert pairwise-distinct kinds.
        let list_req = Request::builder()
            .method("GET")
            .uri("/api/v1/runs?page%5Blimit%5D=100")
            .body(Body::empty())
            .unwrap();
        let list = app.oneshot(list_req).await.unwrap();
        let body = body_value(list).await;
        let data = body["data"].as_array().expect("data array");
        assert!(
            data.len() >= 3,
            "should have ≥3 runs, got {}: {body}",
            data.len()
        );

        // Index by the test's marker. The exact three rows are not
        // guaranteed to be the only ones; filter to the three we
        // created.
        let by_kind: std::collections::HashMap<String, &serde_json::Value> = data
            .iter()
            .filter_map(|row| {
                row["origin"]["kind"]
                    .as_str()
                    .map(|kind| (kind.to_string(), row))
            })
            .fold(std::collections::HashMap::new(), |mut acc, (k, v)| {
                if matches!(k.as_str(), "gate" | "referee" | "referee_backfill") {
                    acc.insert(k, v);
                }
                acc
            });

        assert!(
            by_kind.contains_key("gate"),
            "no gate row in /runs; collapse likely"
        );
        assert!(
            by_kind.contains_key("referee"),
            "no referee row in /runs; collapse likely"
        );
        assert!(
            by_kind.contains_key("referee_backfill"),
            "no referee_backfill row in /runs; collapse likely"
        );

        // The backfill row uniquely carries `backfill_at` at the
        // details level. The other two must NOT.
        let gate = &by_kind["gate"];
        let referee = &by_kind["referee"];
        let backfill = &by_kind["referee_backfill"];

        assert!(
            gate["origin"]["details"]["backfill_at"].is_null(),
            "gate row must NOT carry backfill_at; got={gate}"
        );
        assert!(
            referee["origin"]["details"]["backfill_at"].is_null(),
            "referee row must NOT carry backfill_at; got={referee}"
        );
        assert!(
            !backfill["origin"]["details"]["backfill_at"].is_null(),
            "referee_backfill row MUST carry backfill_at; got={backfill}"
        );

        // And the `kind` field on each row is the one we sent.
        assert_eq!(gate["origin"]["kind"].as_str(), Some("gate"));
        assert_eq!(referee["origin"]["kind"].as_str(), Some("referee"));
        assert_eq!(
            backfill["origin"]["kind"].as_str(),
            Some("referee_backfill")
        );
    }

    #[tokio::test]
    async fn inconclusive_verdict_creates_a_failed_run_not_a_pass() {
        // The "never default to pass" invariant. An Inconclusive verdict
        // maps to `Failed { TransientInfra }`, never to `Succeeded`.
        // If a future refactor maps Inconclusive → Succeeded, this
        // test fails.
        let state = test_app_state();
        let app = build_test_router(state);
        let body = serde_json::to_value(gate_origin(RefereeVerdict::Inconclusive)).unwrap();
        let run_id = sample_run_id("inconclusive");
        let response = app.oneshot(post_registration(body, &run_id)).await.unwrap();
        let body = body_value(response).await;
        let status_kind = body["lifecycle"]["status"]["kind"].as_str();
        assert_eq!(
            status_kind,
            Some("failed"),
            "Inconclusive must produce a failed run, not a passed one; got={body}"
        );
        // And the failed reason is TransientInfra, not WorkflowError.
        let reason = body["lifecycle"]["status"]["reason"].as_str();
        assert_eq!(
            reason,
            Some("transient_infra"),
            "Inconclusive reason should be transient_infra, got {reason:?}"
        );
    }

    #[tokio::test]
    async fn pass_verdict_creates_a_succeeded_run() {
        // Mirror of the above: a real Pass maps to Succeeded/Completed.
        let state = test_app_state();
        let app = build_test_router(state);
        let body = serde_json::to_value(gate_origin(RefereeVerdict::Pass)).unwrap();
        let run_id = sample_run_id("pass");
        let response = app.oneshot(post_registration(body, &run_id)).await.unwrap();
        let body = body_value(response).await;
        let status_kind = body["lifecycle"]["status"]["kind"].as_str();
        assert_eq!(status_kind, Some("succeeded"));
    }
}
