//! `RunOrigin` provenance — the source of a registered run.
//!
//! Three sources of registered runs must remain distinguishable forever so the
//! `/runs` pane never collapses a live gate execution into a hermetic referee
//! score or a backfill retro-score. Each kind lives behind a different variant
//! of [`RunOriginDetails`]; the `kind` tag on `RunOrigin` is the primary
//! discriminator the UI surfaces, and the per-kind payload is the secondary
//! signal that survives the schema bump (e.g. backfill rows uniquely carry
//! `backfill_at`).
//!
//! The contract is intentionally `*` (no `Option`) for the per-kind fields
//! that are universal to that source: a referee row always has a `base_sha`
//! and a `tier`, a gate row always has an `endpoint`, etc. Optional fields
//! are documented individually.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

/// The verdict a gate / referee / backfill source supplied.
///
/// `Inconclusive` is the only honest answer when the source failed to produce
/// a real verdict (timeout, scorer crashed, infra error). Recording a `null`
/// verdict as `Pass` here is the failure mode this whole plane exists to
/// prevent.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RefereeVerdict {
    Pass,
    Fail,
    Inconclusive,
}

/// How the original dispatch call obtained the run_id and base_sha. The
/// SessionEnd hook's `--auto-detect` mode is the dominant path for live
/// referee scores; `--branch`/`--base-ref` are explicit reproductions used
/// by tests and crash diagnostics. Backfill always reports
/// `RefereeBackfill`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RefereeDispatchPath {
    /// Live: hook walked `git rev-parse --abbrev-ref HEAD` and
    /// `git merge-base main HEAD` itself.
    AutoDetect,
    /// Live: caller passed `--branch` and `--base-ref` explicitly (tests,
    /// crash diagnostics, manual reruns).
    Explicit,
    /// Backfill: produced by `scripts/backfill-referee.sh` replaying a
    /// historical attempt.
    RefereeBackfill,
}

/// Source-specific payload for a `gate` origin (live forkd gate execution).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunGateOrigin {
    /// The forkd endpoint that produced the verdict (e.g.
    /// `http://forkd.internal.example:8891`).
    pub endpoint:    String,
    /// The forkd sandbox id this run was executed in.
    pub sandbox_id:  String,
    /// The model the gate ran (e.g. `claude-sonnet-5[1m]`).
    pub model:       String,
    /// The gate's verdict. `Inconclusive` is encoded as `Pass = false` plus
    /// the `error` field populated; a missing `error` with `verdict` here
    /// is a real `Pass` or `Fail`.
    pub verdict:     RefereeVerdict,
    /// When the gate emitted the verdict.
    pub verdict_at:  DateTime<Utc>,
    /// Optional numeric score the gate reported (parsers vary by backend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score:       Option<f64>,
    /// Optional valset hash the gate reported (e.g. `sha256:...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valset_hash: Option<String>,
    /// Free-form `gate_log` excerpt. Stored at registration time so the
    /// registered run carries the same signal the dispatcher saw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_log:    Option<String>,
    /// Populated when `verdict = Inconclusive`. The textual error from the
    /// gate. `None` for real `Pass` / `Fail` verdicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error:       Option<String>,
}

/// Source-specific payload for a `referee` origin (live hermetic SessionEnd
/// referee score).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRefereeOrigin {
    /// The route tier (mm / sn / qw). Always present.
    pub tier:          String,
    /// SHA the diff was taken against. 40 hex chars — name and document.
    /// The handler rejects non-SHA values on the inbound request.
    pub base_sha:      String,
    /// The session's HEAD SHA at dispatch time. 40 hex chars.
    pub head_sha:      String,
    pub verdict:       RefereeVerdict,
    pub verdict_at:    DateTime<Utc>,
    /// How the dispatch script was invoked. `AutoDetect` is the
    /// SessionEnd-hook path; `Explicit` is the
    /// `--branch`/`--base-ref`/test path.
    pub dispatch_path: RefereeDispatchPath,
    /// The branch the route was launched on (e.g. `p0-canary-mm`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch:        Option<String>,
    /// `true` iff the source caller tagged this run as a synthetic proof /
    /// training test (NOT real fleet data). Downstream consumers
    /// (cal trainset, dashboards) MUST filter these out of operational
    /// ground truth.
    #[serde(default)]
    pub synthetic:     bool,
    /// Optional numeric score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score:         Option<f64>,
    /// Optional valset hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valset_hash:   Option<String>,
    /// Populated when `verdict = Inconclusive`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error:         Option<String>,
}

/// Source-specific payload for a `referee_backfill` origin. Adds the
/// backfill-only fields on top of all the `referee` fields; structurally
/// identical to `RunRefereeOrigin` plus `backfill_at` and `requester`, so
/// the two can never be confused by a code reader who only sees the keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRefereeBackfillOrigin {
    /// All the live-referee fields. The duplication is deliberate: the
    /// backfill payload is a superset, not a derivation, so a refactor
    /// that drops `backfill_at` cannot silently keep `RunRefereeOrigin`
    /// compatible.
    pub tier:          String,
    pub base_sha:      String,
    pub head_sha:      String,
    pub verdict:       RefereeVerdict,
    pub verdict_at:    DateTime<Utc>,
    pub dispatch_path: RefereeDispatchPath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch:        Option<String>,
    #[serde(default)]
    pub synthetic:     bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score:         Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valset_hash:   Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error:         Option<String>,
    /// When the backfill retro-score was produced. The discriminator
    /// between a live referee score and a backfill row is the presence of
    /// this field; the handler treats it as required for the
    /// `referee_backfill` origin kind.
    pub backfill_at:   DateTime<Utc>,
    /// Who or what produced the backfill row (e.g. `backfill-referee.sh`,
    /// `scripts/backfill-referee.sh`, or an operator identifier for a
    /// manual rerun).
    pub requester:     String,
}

/// Per-kind payload for a non-`api` origin. The `kind` tag on `RunOrigin`
/// is the primary discriminator; the variant here is the secondary signal
/// that survives a `kind` typo (the handler rejects mismatched kinds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunOriginDetails {
    Gate(RunGateOrigin),
    Referee(RunRefereeOrigin),
    RefereeBackfill(RunRefereeBackfillOrigin),
}

impl RunOriginDetails {
    #[must_use]
    pub fn kind(&self) -> super::RunOriginKind {
        match self {
            Self::Gate(_) => super::RunOriginKind::Gate,
            Self::Referee(_) => super::RunOriginKind::Referee,
            Self::RefereeBackfill(_) => super::RunOriginKind::RefereeBackfill,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_kind_discriminator_survives_round_trip() {
        // The single test that protects the "three sources stay
        // distinguishable" property at the type layer. If a future
        // refactor maps every payload to the same struct, the
        // `tag = "kind"` rename_all `snake_case` discriminator stops
        // disambiguating and the deserialization below will panic.
        let gate = RunOriginDetails::Gate(RunGateOrigin {
            endpoint:    "http://forkd.internal.example:8891".to_string(),
            sandbox_id:  "sb-1".to_string(),
            model:       "claude-sonnet-5[1m]".to_string(),
            verdict:     RefereeVerdict::Pass,
            verdict_at:  Utc::now(),
            score:       None,
            valset_hash: None,
            gate_log:    Some("ok".to_string()),
            error:       None,
        });
        let referee = RunOriginDetails::Referee(RunRefereeOrigin {
            tier:          "minimax".to_string(),
            base_sha:      "a".repeat(40),
            head_sha:      "b".repeat(40),
            verdict:       RefereeVerdict::Pass,
            verdict_at:    Utc::now(),
            dispatch_path: RefereeDispatchPath::AutoDetect,
            branch:        Some("T-x-mm".to_string()),
            synthetic:     false,
            score:         None,
            valset_hash:   None,
            error:         None,
        });
        let backfill = RunOriginDetails::RefereeBackfill(RunRefereeBackfillOrigin {
            tier:          "sonnet".to_string(),
            base_sha:      "c".repeat(40),
            head_sha:      "d".repeat(40),
            verdict:       RefereeVerdict::Fail,
            verdict_at:    Utc::now(),
            dispatch_path: RefereeDispatchPath::RefereeBackfill,
            branch:        Some("p0-canary-sn".to_string()),
            synthetic:     false,
            score:         None,
            valset_hash:   None,
            error:         None,
            backfill_at:   Utc::now(),
            requester:     "backfill-referee.sh".to_string(),
        });

        for (label, origin) in [
            ("gate", &gate),
            ("referee", &referee),
            ("referee_backfill", &backfill),
        ] {
            let json = serde_json::to_string(origin).expect("serialize");
            let parsed: RunOriginDetails = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{label}: round-trip parse failed: {e}; json={json}"));
            assert_eq!(
                parsed.kind(),
                origin.kind(),
                "{label}: kind discriminator must survive JSON round-trip"
            );
        }
    }

    #[test]
    fn backfill_origin_uniquely_carries_backfill_at() {
        // Belt-and-braces: a gate row's decoded JSON must NOT deserialize
        // into a `RefereeBackfill` variant because the `backfill_at` field
        // is missing. This guards against a future schema where the
        // backfill-only fields are absorbed into the live type and the
        // hand-rolled discrimination is silently lost.
        let gate_json = serde_json::to_string(&RunOriginDetails::Gate(RunGateOrigin {
            endpoint:    "http://forkd.internal.example:8891".to_string(),
            sandbox_id:  "sb-1".to_string(),
            model:       "claude-sonnet-5[1m]".to_string(),
            verdict:     RefereeVerdict::Pass,
            verdict_at:  Utc::now(),
            score:       None,
            valset_hash: None,
            gate_log:    None,
            error:       None,
        }))
        .unwrap();
        let parsed: RunOriginDetails = serde_json::from_str(&gate_json).unwrap();
        // Gate must NOT be a RefereeBackfill — collapse check.
        assert!(matches!(parsed, RunOriginDetails::Gate(_)));
        assert!(!matches!(parsed, RunOriginDetails::RefereeBackfill(_)));
    }

    #[test]
    fn inconclusive_is_a_real_verdict_not_pass() {
        // The never-default-to-pass invariant.
        let ref_rank: u8 = match RefereeVerdict::Inconclusive {
            RefereeVerdict::Pass | RefereeVerdict::Fail => 0,
            RefereeVerdict::Inconclusive => 1,
        };
        assert_eq!(
            ref_rank, 1,
            "Inconclusive must be a distinct verdict; the handler must NOT coerce it to Pass"
        );
    }
}
