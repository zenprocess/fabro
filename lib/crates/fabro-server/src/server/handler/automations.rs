use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query as ExtraQuery;
use chrono::Utc;
use fabro_automation::{
    ApiTrigger, Automation, AutomationDraft, AutomationId, AutomationPatch, AutomationReplace,
    AutomationRevision, AutomationStoreError, AutomationTarget, AutomationTrigger,
    AutomationTriggerId, AutomationValidationError, GitRefSelector, RepositorySlug,
    ScheduleTrigger, WorkflowSlug,
};
use fabro_types::{AutomationRef, RunId};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::super::{AppState, PaginationParams, paginate_items, resolve_interp_string};
use super::runs::{CreateRunFromManifestRequest, create_run_from_manifest};
use crate::automation_materializer::{
    AutomationRunMaterializeError, AutomationRunMaterializeInput, automation_temp_root,
};
use crate::error::ApiError;
use crate::principal_middleware::{RequiredRunManagementActor, RequiredUser};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/automations",
            get(list_automations).post(create_automation),
        )
        .route(
            "/automations/{id}",
            get(get_automation)
                .put(replace_automation)
                .patch(patch_automation)
                .delete(delete_automation),
        )
        .route(
            "/automations/{id}/runs",
            get(list_automation_runs).post(create_automation_run),
        )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAutomationTarget {
    repository: String,
    #[serde(rename = "ref")]
    ref_:       String,
    workflow:   String,
}

#[derive(Debug, Deserialize)]
struct RawAutomationTrigger {
    id:         String,
    #[serde(rename = "type")]
    type_:      String,
    #[serde(default = "default_true")]
    enabled:    bool,
    #[serde(default)]
    expression: Option<String>,
    #[serde(flatten)]
    extra:      BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCreateAutomationRequest {
    id:          String,
    name:        String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    enabled:     Option<bool>,
    target:      RawAutomationTarget,
    triggers:    Vec<RawAutomationTrigger>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReplaceAutomationRequest {
    name:        String,
    #[serde(default)]
    description: Option<String>,
    enabled:     bool,
    target:      RawAutomationTarget,
    triggers:    Vec<RawAutomationTrigger>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawPatchAutomationRequest {
    #[serde(default)]
    name:        Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_string_patch")]
    description: NullableStringPatch,
    #[serde(default)]
    enabled:     Option<bool>,
    #[serde(default)]
    target:      Option<RawAutomationTarget>,
    #[serde(default)]
    triggers:    Option<Vec<RawAutomationTrigger>>,
}

#[derive(Debug, Default)]
enum NullableStringPatch {
    #[default]
    Omitted,
    Explicit(Option<String>),
}

impl NullableStringPatch {
    fn apply_to(self, patch: &mut AutomationPatch) {
        match self {
            Self::Omitted => {}
            Self::Explicit(value) => patch.description = Some(value),
        }
    }
}

fn deserialize_nullable_string_patch<'de, D>(
    deserializer: D,
) -> Result<NullableStringPatch, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(NullableStringPatch::Explicit)
}

#[derive(serde::Serialize)]
struct AutomationListResponse {
    data: Vec<Automation>,
    meta: AutomationListMeta,
}

#[derive(serde::Serialize)]
struct AutomationListMeta {
    total: u64,
}

fn default_true() -> bool {
    true
}

async fn list_automations(_auth: RequiredUser, State(state): State<Arc<AppState>>) -> Response {
    // `AutomationStore::list` already yields entries in `AutomationId` order
    // (BTreeMap iteration); no additional sort is required.
    let automations = state.automation_store().list().await;
    let total = automations.len() as u64;
    (
        StatusCode::OK,
        Json(AutomationListResponse {
            data: automations,
            meta: AutomationListMeta { total },
        }),
    )
        .into_response()
}

async fn create_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Response {
    let request = match parse_json::<RawCreateAutomationRequest>(&body) {
        Ok(request) => request,
        Err(err) => return err.into_response(),
    };
    let draft = match request.try_into() {
        Ok(draft) => draft,
        Err(err) => return validation_error(&err).into_response(),
    };
    match state.automation_store().create(draft).await {
        Ok(automation) => (StatusCode::CREATED, Json(automation)).into_response(),
        Err(err) => store_error(err).into_response(),
    }
}

async fn get_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let id = match parse_automation_id(&id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    match state.automation_store().get(&id).await {
        Some(automation) => with_etag(StatusCode::OK, automation),
        None => ApiError::not_found("Automation not found.").into_response(),
    }
}

async fn replace_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let id = match parse_automation_id(&id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    let expected = match parse_if_match(&headers) {
        Ok(revision) => revision,
        Err(err) => return err.into_response(),
    };
    let request = match parse_json::<RawReplaceAutomationRequest>(&body) {
        Ok(request) => request,
        Err(err) => return err.into_response(),
    };
    let draft = match request.try_into() {
        Ok(draft) => draft,
        Err(err) => return validation_error(&err).into_response(),
    };
    match state
        .automation_store()
        .replace(&id, &expected, draft)
        .await
    {
        Ok(automation) => with_etag(StatusCode::OK, automation),
        Err(err) => store_error(err).into_response(),
    }
}

async fn patch_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let id = match parse_automation_id(&id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    let expected = match parse_if_match(&headers) {
        Ok(revision) => revision,
        Err(err) => return err.into_response(),
    };
    let request = match parse_json::<RawPatchAutomationRequest>(&body) {
        Ok(request) => request,
        Err(err) => return err.into_response(),
    };
    let patch = match request.try_into() {
        Ok(patch) => patch,
        Err(err) => return validation_error(&err).into_response(),
    };
    match state.automation_store().patch(&id, &expected, patch).await {
        Ok(automation) => with_etag(StatusCode::OK, automation),
        Err(err) => store_error(err).into_response(),
    }
}

async fn delete_automation(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let id = match parse_automation_id(&id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    let expected = match parse_if_match(&headers) {
        Ok(revision) => revision,
        Err(err) => return err.into_response(),
    };
    match state.automation_store().delete(&id, &expected).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => store_error(err).into_response(),
    }
}

async fn list_automation_runs(
    _auth: RequiredUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    ExtraQuery(pagination): ExtraQuery<PaginationParams>,
) -> Response {
    let id = match parse_automation_id(&id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    if state.automation_store().get(&id).await.is_none() {
        return ApiError::not_found("Automation not found.").into_response();
    }
    let entries = match state
        .store_ref()
        .list_cached_runs(&fabro_store::ListRunsQuery::default(), Utc::now())
        .await
    {
        Ok(entries) => entries,
        Err(err) => {
            return ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response();
        }
    };
    let mut runs = entries
        .into_iter()
        .map(|entry| entry.summary)
        .filter(|run| {
            run.automation
                .as_ref()
                .is_some_and(|automation| automation.id == id.as_str())
        })
        .collect::<Vec<_>>();
    runs.sort_by(|left, right| {
        right
            .timestamps
            .created_at
            .cmp(&left.timestamps.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    let total = runs.len() as u64;
    let decorated = state.decorate_run_summaries(runs).await;
    let (data, has_more) = paginate_items(decorated, &pagination);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "data": data,
            "meta": { "has_more": has_more, "total": total }
        })),
    )
        .into_response()
}

async fn create_automation_run(
    RequiredRunManagementActor(actor): RequiredRunManagementActor,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let id = match parse_automation_id(&id) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    let Some(automation) = state.automation_store().get(&id).await else {
        return ApiError::not_found("Automation not found.").into_response();
    };
    let Some(api_trigger) = startable_api_trigger(&automation) else {
        return ApiError::with_code(
            StatusCode::CONFLICT,
            "Automation has no enabled API trigger.",
            "automation_api_trigger_disabled",
        )
        .into_response();
    };

    let run_id = RunId::new();
    let storage_root = match resolve_interp_string(&state.server_settings().server.storage.root) {
        Ok(path) => path,
        Err(err) => {
            return ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to resolve server storage root: {err}"),
            )
            .into_response();
        }
    };
    let materialized = match state
        .automation_materializer()
        .materialize(AutomationRunMaterializeInput {
            target: automation.target.clone(),
            run_id,
            user_settings_path: state.active_config_path().to_path_buf(),
            temp_root: automation_temp_root(storage_root),
        })
        .await
    {
        Ok(materialized) => materialized,
        Err(err) => return materialize_error(&err).into_response(),
    };

    let automation_ref = AutomationRef {
        id:         id.to_string(),
        name:       Some(automation.name.clone()),
        trigger_id: Some(api_trigger.id.to_string()),
    };
    Box::pin(create_run_from_manifest(
        state,
        CreateRunFromManifestRequest {
            explicit_title_supplied: materialized.manifest.title.is_some(),
            manifest: materialized.manifest,
            submitted_manifest_bytes: materialized.submitted_manifest_bytes,
            explicit_run_id: Some(run_id),
            actor,
            headers,
            automation: Some(automation_ref),
        },
    ))
    .await
}

fn startable_api_trigger(automation: &Automation) -> Option<&ApiTrigger> {
    if !automation.enabled {
        return None;
    }
    automation
        .triggers
        .iter()
        .find_map(|trigger| match trigger {
            AutomationTrigger::Api(trigger) if trigger.enabled => Some(trigger),
            AutomationTrigger::Api(_) | AutomationTrigger::Schedule(_) => None,
        })
}

fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|err| ApiError::bad_request(err.to_string()))
}

fn parse_automation_id(value: &str) -> Result<AutomationId, ApiError> {
    AutomationId::try_from(value.to_string()).map_err(|err| ApiError::bad_request(err.to_string()))
}

fn parse_if_match(headers: &HeaderMap) -> Result<AutomationRevision, ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Err(ApiError::new(
            StatusCode::PRECONDITION_REQUIRED,
            "If-Match header is required.",
        ));
    };
    let value = value
        .to_str()
        .map_err(|err| ApiError::bad_request(format!("Invalid If-Match header: {err}")))?
        .trim();
    let revision = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .trim();
    if revision.is_empty() {
        return Err(ApiError::bad_request(
            "If-Match revision must not be empty.",
        ));
    }
    Ok(AutomationRevision::from_raw(revision))
}

fn with_etag(status: StatusCode, automation: Automation) -> Response {
    let etag = format!("\"{}\"", automation.revision);
    let etag = HeaderValue::from_str(&etag).expect("revision etag should be a valid header value");
    (status, [(header::ETAG, etag)], Json(automation)).into_response()
}

fn validation_error(err: &AutomationValidationError) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, err.to_string())
}

fn store_error(err: AutomationStoreError) -> ApiError {
    match err {
        AutomationStoreError::NotFound(_) => ApiError::not_found("Automation not found."),
        AutomationStoreError::AlreadyExists(_) => {
            ApiError::new(StatusCode::CONFLICT, "Automation already exists.")
        }
        AutomationStoreError::RevisionMismatch { .. } => {
            ApiError::new(StatusCode::CONFLICT, "Automation revision mismatch.")
        }
        AutomationStoreError::Validation(err) => validation_error(&err),
        AutomationStoreError::Parse { .. }
        | AutomationStoreError::Serialize(_)
        | AutomationStoreError::Io { .. } => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        }
    }
}

fn materialize_error(err: &AutomationRunMaterializeError) -> ApiError {
    // All current variants surface as 422 — they describe automation
    // misconfiguration or repository state that the caller can correct.
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, err.to_string())
}

impl TryFrom<RawAutomationTarget> for AutomationTarget {
    type Error = AutomationValidationError;

    fn try_from(value: RawAutomationTarget) -> Result<Self, Self::Error> {
        Ok(Self {
            repository: RepositorySlug::try_from(value.repository)?,
            ref_:       GitRefSelector::try_from(value.ref_)?,
            workflow:   WorkflowSlug::try_from(value.workflow)?,
        })
    }
}

impl TryFrom<RawAutomationTrigger> for AutomationTrigger {
    type Error = AutomationValidationError;

    fn try_from(value: RawAutomationTrigger) -> Result<Self, Self::Error> {
        let id = AutomationTriggerId::try_from(value.id)?;
        match value.type_.as_str() {
            "api" => {
                reject_trigger_shape(
                    value.expression.is_some() || !value.extra.is_empty(),
                    "api trigger only supports id, type, and enabled",
                )?;
                Ok(Self::Api(ApiTrigger {
                    id,
                    enabled: value.enabled,
                }))
            }
            "schedule" => {
                reject_trigger_shape(
                    !value.extra.is_empty(),
                    "schedule trigger only supports id, type, enabled, and expression",
                )?;
                Ok(Self::Schedule(ScheduleTrigger {
                    id,
                    enabled: value.enabled,
                    expression: value.expression.unwrap_or_default(),
                }))
            }
            _ => Err(AutomationValidationError::UnknownTriggerType(value.type_)),
        }
    }
}

fn reject_trigger_shape(
    invalid: bool,
    message: &'static str,
) -> Result<(), AutomationValidationError> {
    if invalid {
        Err(AutomationValidationError::InvalidTriggerShape(
            message.to_string(),
        ))
    } else {
        Ok(())
    }
}

impl TryFrom<RawCreateAutomationRequest> for AutomationDraft {
    type Error = AutomationValidationError;

    fn try_from(value: RawCreateAutomationRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            id:          AutomationId::try_from(value.id)?,
            name:        value.name,
            description: value.description,
            enabled:     value.enabled,
            target:      value.target.try_into()?,
            triggers:    convert_triggers(value.triggers)?,
        })
    }
}

impl TryFrom<RawReplaceAutomationRequest> for AutomationReplace {
    type Error = AutomationValidationError;

    fn try_from(value: RawReplaceAutomationRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            name:        value.name,
            description: value.description,
            enabled:     value.enabled,
            target:      value.target.try_into()?,
            triggers:    convert_triggers(value.triggers)?,
        })
    }
}

impl TryFrom<RawPatchAutomationRequest> for AutomationPatch {
    type Error = AutomationValidationError;

    fn try_from(value: RawPatchAutomationRequest) -> Result<Self, Self::Error> {
        let mut patch = Self {
            name:        value.name,
            description: None,
            enabled:     value.enabled,
            target:      value.target.map(TryInto::try_into).transpose()?,
            triggers:    value.triggers.map(convert_triggers).transpose()?,
        };
        value.description.apply_to(&mut patch);
        Ok(patch)
    }
}

fn convert_triggers(
    triggers: Vec<RawAutomationTrigger>,
) -> Result<Vec<AutomationTrigger>, AutomationValidationError> {
    triggers.into_iter().map(TryInto::try_into).collect()
}
