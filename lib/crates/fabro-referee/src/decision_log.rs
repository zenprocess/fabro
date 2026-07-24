//! Parser for `~/.ao/data/wrapper-decisions.jsonl` — the routing
//! decision log the `claude-wrapper.sh` (v4.0, 2026-07-20) writes.
//!
//! The wrapper emits one JSON object per line, immediately before each
//! fleet exec, with the routing TREATMENT (the decision + its basis).
//! The scorer joins on `session_id` + `branch` to recover the *real*
//! `tier_resolved` + `decision_basis` for a route — the goal (the
//! branch suffix) and the actual (what the wrapper ran) can diverge
//! when the wrapper diverts (e.g. minimax quota → forced sonnet).
//!
//! Wire contract (one line per dispatch, see `claude-wrapper.sh:80`):
//!
//! ```json
//! {
//!   "ts": "2026-07-23T10:00:00Z",
//!   "session_id": "ao-75",
//!   "role": "worker|orchestrator|reviewer",
//!   "tier_resolved": "minimax|sonnet|qwen|opus|cloud",
//!   "decision_basis": "branch-suffix|...|cloud-default|role",
//!   "branch": "<t>-mm|<t>-sn|<t>-qw|...",
//!   "reqmodel": "...",
//!   "minimax_preflight_result": "healthy|quota|unreachable|n/a",
//!   "fallback_taken": true|false,
//!   "final_model": "...",
//!   "final_endpoint": "..."
//! }
//! ```
//!
//! Reconstructed fields used by the row:
//!   * `tier_resolved` (Strick → `mm|sn|qw|opus|cloud`) — the actual tier.
//!   * `decision_basis` (Strick) — why the wrapper chose that tier.
//!   * `final_model` (Strick) — the model id the wrapper ran (e.g.
//!     `MiniMax-M3`, `claude-sonnet-5[1m]`). Forwarded into `RunRow.model` to
//!     match zeninfra's `GateLogLine.model`.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One parsed wrapper-decisions.jsonl line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionLogLine {
    /// ISO-8601 UTC timestamp.
    pub ts: String,
    /// The worktree dir name (the wrapper's session id).
    pub session_id: String,
    /// `worker` | `orchestrator` | `reviewer`.
    pub role: String,
    /// The tier the wrapper actually ran on.
    pub tier_resolved: String,
    /// Why the wrapper chose that tier.
    pub decision_basis: String,
    /// The branch the orchestrator passed to `ao spawn`.
    pub branch: String,
    /// The `--model` AO injected from per-project config.
    pub reqmodel: String,
    /// `healthy` | `quota` | `unreachable` | `n/a`.
    pub minimax_preflight_result: String,
    /// Whether the wrapper diverted from the goal tier.
    pub fallback_taken: bool,
    /// The final model id (e.g. `MiniMax-M3`).
    pub final_model: String,
    /// The host the exec ran on (`minimax` | `ccproxy2-daemon` | `cclocal`).
    pub final_endpoint: String,
}

/// Locate a route's decision-log line by `session_id` + `branch`.
/// Tolerant: returns `None` if the file is absent or no line matches.
#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: reads a small append-only JSONL log; no Tokio runtime here"
)]
pub fn find_decision(log_path: &Path, session_id: &str, branch: &str) -> Option<DecisionLogLine> {
    let content = std::fs::read_to_string(log_path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: DecisionLogLine = match serde_json::from_str(line) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if parsed.session_id == session_id && parsed.branch == branch {
            return Some(parsed);
        }
    }
    None
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "sync test fixture: writes a temp JSONL file with std::io::Write"
)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn parses_minimal_line() {
        let raw = r#"{"ts":"2026-07-23T10:00:00Z","session_id":"ao-75","role":"worker","tier_resolved":"minimax","decision_basis":"branch-suffix","branch":"p0-mm","reqmodel":"MiniMax-M3","minimax_preflight_result":"healthy","fallback_taken":false,"final_model":"MiniMax-M3","final_endpoint":"minimax"}"#;
        let line: DecisionLogLine = serde_json::from_str(raw).unwrap();
        assert_eq!(line.tier_resolved, "minimax");
        assert_eq!(line.decision_basis, "branch-suffix");
        assert!(!line.fallback_taken);
    }

    #[test]
    fn find_decision_matches_branch_and_session() {
        let tmp = std::env::temp_dir().join(format!(
            "fabro-referee-find-decision-{}.jsonl",
            ulid::Ulid::new()
        ));
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            writeln!(
                f,
                r#"{{"ts":"2026-07-23T10:00:00Z","session_id":"ao-75","role":"worker","tier_resolved":"minimax","decision_basis":"branch-suffix","branch":"p0-mm","reqmodel":"MiniMax-M3","minimax_preflight_result":"healthy","fallback_taken":false,"final_model":"MiniMax-M3","final_endpoint":"minimax"}}"#
            ).unwrap();
            writeln!(
                f,
                r#"{{"ts":"2026-07-23T10:00:01Z","session_id":"ao-75","role":"worker","tier_resolved":"sonnet","decision_basis":"branch-suffix","branch":"p0-sn","reqmodel":"claude-sonnet-5[1m]","minimax_preflight_result":"n/a","fallback_taken":false,"final_model":"claude-sonnet-5[1m]","final_endpoint":"ccproxy2-daemon"}}"#
            ).unwrap();
        }
        let hit = find_decision(&tmp, "ao-75", "p0-sn").unwrap();
        assert_eq!(hit.tier_resolved, "sonnet");
        let miss = find_decision(&tmp, "ao-75", "p0-does-not-exist");
        assert!(miss.is_none());
        let _ = std::fs::remove_file(&tmp);
    }
}
