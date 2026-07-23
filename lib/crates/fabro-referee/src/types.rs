//! Core data types for the Referee scorer.
//!
//! The scorer is the **Referee plane** of the AO Factory (per
//! `~/.ao/data/aofactory/AO-FACTORY-RECORD.md` §4.2/§4.3/§5.2 P0). It
//! consumes one task spec, fans it across ≥2 model-tier routes on the
//! same `claude-code` harness, scores each route's diff through the
//! forkd hermetic gate, and emits one JSONL row per route into a
//! readable sink. The row shape is a forward-compatible superset of
//! zeninfra's `RefereeScore` (`ao-factory/signal/schema.py`).
//!
//! Forward-compatible superset fields beyond the canonical schema are
//! flagged in [`crate::emit`]; the harvest ETL (zeninfra-side) folds
//! the JSONL into the episode store and decides which fields to land
//! in v1 vs. defer to a schema bump.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use strum::Display;

/// Pass / fail verdict from the gate. The textual feedback GEPA consumes
/// lives in [`GateOutput::gate_log`] regardless of which side of the
/// pass/fail line the attempt landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
}

/// The model tier the orchestrator's branch suffix names — the **goal**
/// route. The wrapper may divert (e.g. minimax quota → forced sonnet
/// fallback); the *actual* tier that ran is in [`DecisionLog::tier_resolved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Tier {
    /// minimax M3 (the `-mm` branch suffix).
    Minimax,
    /// claude-sonnet-5[1m] (the `-sn` branch suffix).
    Sonnet,
    /// qwen (the `-qw` branch suffix) — not in P0 canary but supported
    /// by the scorer for symmetry with the wrapper.
    Qwen,
    /// opus / cloud (the orchestrator fallback). Not currently in P0
    /// canary contract.
    Cloud,
}

/// Closed-form acceptance for the canary: a single command the gate
/// runs after applying the diff in a throwaway checkout. Exit 0 → pass,
/// non-zero → fail. The scorer captures combined stdout/stderr as
/// `gate_log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Acceptance {
    /// Run an arbitrary shell command. Used by the canary.
    #[serde(rename = "shell_command")]
    ShellCommand {
        /// The command to run. Already shell-quoted by the caller.
        command: String,
    },
    /// A regex that must match the diff (compiled at score time).
    #[serde(rename = "diff_must_match")]
    DiffMustMatch {
        /// The regex pattern.
        pattern: String,
    },
}

/// One task to be scored. The canary is exactly this struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    /// Operator-stable id (e.g. `T-canary-echo-hello`).
    pub task_id:           String,
    /// Optional path / URL of the spec driving this task. Forwarded
    /// to the episode store's `spec_ref`.
    #[serde(default)]
    pub spec_ref:          Option<String>,
    /// Optional difficulty bucket (e.g. `easy`, `medium`, `hard`).
    /// Forwarded to the episode store's `difficulty_bucket`.
    #[serde(default)]
    pub difficulty_bucket: Option<String>,
    /// Path to the prompt file the orchestrator passes to `ao spawn`.
    pub prompt_path:       String,
    /// Working tree path of the target project (e.g. the fabro worktree).
    pub project_path:      String,
    /// Git ref (branch / SHA) the route's diff is taken against.
    pub base_ref:          String,
    /// The closed-form acceptance the gate runs.
    pub acceptance:        Acceptance,
}

/// One route = one tier attempt on the same task. Constructed by the
/// runner as the orchestrator spawns `ao spawn --branch <t>-mm` /
/// `<t>-sn` and the wrapper logs the routing decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// The goal tier (what the branch suffix named).
    pub tier:           Tier,
    /// The branch the orchestrator asked for (e.g. `p0-canary-mm`).
    pub branch:         String,
    /// The actual tier that ran (from the wrapper-decisions.jsonl).
    /// Differs from `tier` if the wrapper diverted (e.g. quota fallback).
    pub tier_resolved:  Option<String>,
    /// The wrapper's decision basis for the actual tier (e.g.
    /// `branch-suffix`, `reqmodel-match`, `cloud-default`).
    pub decision_basis: Option<String>,
    /// The session id the wrapper recorded (the worktree dir name).
    pub session_id:     Option<String>,
    /// The diff this route produced. Captured by the runner before
    /// handing it to the gate.
    pub diff:           String,
    /// `--stat` output (`files changed, +xx, -yy`) for the row.
    #[serde(default)]
    pub diff_stat:      Option<String>,
}

/// Output from any gate backend. Both backends return this shape so
/// the runner can emit the JSONL row indistinguishably.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateOutput {
    pub verdict:     Verdict,
    /// The textual feedback GEPA consumes. Never empty.
    pub gate_log:    String,
    /// Free-form backend name (e.g. `forkd`, `hermetic`, `fake`).
    pub backend:     String,
    /// Optional numeric score (parsers such as `gate_score` may be
    /// `None`; the field is intentionally free-form to absorb the
    /// zeninfra `RefereeScore.score` shape).
    #[serde(default)]
    pub score:       Option<f64>,
    /// Optional valset hash (`sha256:...`). The hermetic backend
    /// computes the hash of the canned diff + task id; the forkd
    /// backend reports whatever the controller reports.
    #[serde(default)]
    pub valset_hash: Option<String>,
}

/// One JSONL row — one route, one task, one scored gate-log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRow {
    /// Schema version of this row. Starts at 1. The zeninfra
    /// episode-store sink version-gates harvest on this field so a
    /// future shape change does not silently re-land older rows.
    /// Bump on backwards-incompatible changes; additive fields
    /// (Option / serde-default) only need a comment.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub run_id:         String,
    pub task_id:        String,
    pub ts:             DateTime<Utc>,
    /// `"mm"` or `"sn"` — the route shorthand.
    pub route:          String,
    /// The `tier` field as a string (mirrors `tier`).
    pub tier:           String,
    /// The actual tier that ran (may diverge from `tier` on fallback).
    pub tier_resolved:  Option<String>,
    /// The wrapper's decision basis for the actual tier.
    pub decision_basis: Option<String>,
    /// The harness name (always `claude-code` for P0).
    pub harness:        String,
    /// The branch the route was launched on.
    pub branch:         String,
    pub verdict:        Verdict,
    /// `"forkd"` | `"hermetic"` | `"fake"` — which backend fired.
    pub gate_backend:   String,
    /// The textual feedback GEPA consumes.
    pub gate_log:       String,
    /// Optional numeric score.
    #[serde(default)]
    pub score:          Option<f64>,
    /// Optional valset hash.
    #[serde(default)]
    pub valset_hash:    Option<String>,
    /// `git diff --stat` summary.
    #[serde(default)]
    pub diff_stat:      Option<String>,
    /// The wrapper-decision-log session id (worktree dir name).
    #[serde(default)]
    pub session_id:     Option<String>,
}

/// Current schema version. Bump on backwards-incompatible row-shape
/// changes.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

impl RunRow {
    /// Convenience constructor for tests. Always emits the current
    /// schema version so the row shape stays forward-compatible.
    pub fn new(
        run_id: &str,
        task_id: &str,
        route: &str,
        tier: &str,
        branch: &str,
        verdict: Verdict,
        backend: &str,
        gate_log: &str,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            run_id: run_id.to_string(),
            task_id: task_id.to_string(),
            ts: Utc::now(),
            route: route.to_string(),
            tier: tier.to_string(),
            tier_resolved: None,
            decision_basis: None,
            harness: "claude-code".to_string(),
            branch: branch.to_string(),
            verdict,
            gate_backend: backend.to_string(),
            gate_log: gate_log.to_string(),
            score: None,
            valset_hash: None,
            diff_stat: None,
            session_id: None,
        }
    }
}
