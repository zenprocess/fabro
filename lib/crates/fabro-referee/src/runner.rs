//! The runner — the three-stage seam chained for one task × N tiers.
//!
//! Stage 1: `run-routes` — for each tier (`mm`, `sn`), either invoke
//! `ao spawn` (live canary) or use a pre-captured `route_diff`
//! (hermetic tests). Output: `Vec<Route>`.
//!
//! Stage 2: `gate` — for each route, call the configured `GateBackend`
//! and collect `GateOutput`. Output: `Vec<(Route, GateOutput)>`.
//!
//! Stage 3: `emit` — fold the per-route outcomes into `RunRow`s and
//! write to the JSONL sink + Markdown summary. The `run_id` is the
//! orchestrator's grouping key (one canary = one `run_id`).
//!
//! The runner is the **only** place that knows about all three
//! stages. Stages 1 and 2 are independently testable: the hermetic
//! tests exercise stage 2 (gate) via canned diffs + the
//! `FakeBackend`; the live canary exercises stage 1 (`ao spawn`)
//! from the orchestrator's host.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{info, warn};

use crate::decision_log::find_decision;
use crate::emit::{append_jsonl, write_markdown_summary};
use crate::gate::GateBackend;
use crate::types::{CURRENT_SCHEMA_VERSION, GateOutput, Route, RunRow, TaskSpec, Tier, Verdict};

/// What the runner emits for one task.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub run_id: String,
    pub rows:   Vec<RunRow>,
}

/// Run the three-stage chain for one task against the supplied
/// routes. The runner does NOT spawn `ao` itself — the caller
/// supplies the routes' diffs + tier names. This keeps the runner
/// test-friendly (a stub `ao spawn` is trivial) and the orchestrator
/// in control of the actual spawn.
pub fn run(
    task: &TaskSpec,
    routes: &[Route],
    gate: &dyn GateBackend,
    sink_dir: &Path,
    run_id: &str,
    decision_log: Option<&Path>,
) -> Result<RunResult> {
    let mut rows: Vec<RunRow> = Vec::with_capacity(routes.len());
    for route in routes {
        info!(
            task = %task.task_id,
            branch = %route.branch,
            tier = %route.tier,
            "scoring route"
        );
        let mut route = route.clone();
        // If a decision-log path was supplied, attempt to recover
        // the wrapper's actual `tier_resolved` + `decision_basis` +
        // `final_model` (the model's name flows into the row as
        // `model` to match zeninfra's `GateLogLine.model`).
        if let (Some(path), Some(sid)) = (decision_log, &route.session_id) {
            match find_decision(path, sid, &route.branch) {
                Some(hit) => {
                    route.tier_resolved = Some(hit.tier_resolved);
                    route.decision_basis = Some(hit.decision_basis);
                    route.model = Some(hit.final_model);
                }
                None => {
                    warn!(
                        session_id = %sid,
                        branch = %route.branch,
                        "no decision-log line matched; emitting route with goal-tier only"
                    );
                }
            }
        }
        let gate_out = gate
            .score(task, &route.diff)
            .with_context(|| format!("gate.score for branch={}", route.branch))?;
        let row = make_row(run_id, task, &route, &gate_out);
        append_jsonl(sink_dir, &row)
            .with_context(|| format!("emit jsonl for branch={}", route.branch))?;
        rows.push(row);
    }
    write_markdown_summary(sink_dir, run_id, &rows).with_context(|| "emit markdown summary")?;
    info!(
        run_id = %run_id,
        task = %task.task_id,
        rows = rows.len(),
        "run complete"
    );
    Ok(RunResult {
        run_id: run_id.to_string(),
        rows,
    })
}

/// Build a `RunRow` from one route + its gate output. The helper is
/// pure / dead-simple so a reviewer can verify the row shape in one
/// read.
pub fn make_row(run_id: &str, task: &TaskSpec, route: &Route, gate: &GateOutput) -> RunRow {
    let route_short = route_short(route.tier);
    RunRow {
        schema_version: CURRENT_SCHEMA_VERSION,
        run_id:         run_id.to_string(),
        task_id:        task.task_id.clone(),
        attempt_key:    format!("{}#{}#{}", task.task_id, run_id, route_short),
        ts:             Utc::now(),
        route:          route_short,
        tier:           route.tier.to_string(),
        tier_resolved:  route.tier_resolved.clone(),
        decision_basis: route.decision_basis.clone(),
        harness:        "claude-code".to_string(),
        model:          route.model.clone(),
        branch:         route.branch.clone(),
        verdict:        gate.verdict,
        passed:         matches!(gate.verdict, Verdict::Pass),
        gate_backend:   gate.backend.clone(),
        gate_log:       gate.gate_log.clone(),
        score:          gate.score,
        valset_hash:    gate.valset_hash.clone(),
        diff_stat:      route.diff_stat.clone(),
        session_id:     route.session_id.clone(),
        synthetic:      task.synthetic,
    }
}

fn route_short(tier: Tier) -> String {
    match tier {
        Tier::Minimax => "mm".to_string(),
        Tier::Sonnet => "sn".to_string(),
        Tier::Qwen => "qw".to_string(),
        Tier::Cloud => "cloud".to_string(),
    }
}

/// The tier intent & branch name for the standard 2-tier canary.
pub fn two_tier_canary_routes(base_branch: &str, mm_diff: String, sn_diff: String) -> Vec<Route> {
    let mm_branch = format!("{base_branch}-mm");
    let sn_branch = format!("{base_branch}-sn");
    vec![
        Route {
            tier:           Tier::Minimax,
            branch:         mm_branch,
            tier_resolved:  None,
            decision_basis: None,
            model:          None,
            session_id:     None,
            diff:           mm_diff,
            diff_stat:      None,
        },
        Route {
            tier:           Tier::Sonnet,
            branch:         sn_branch,
            tier_resolved:  None,
            decision_basis: None,
            model:          None,
            session_id:     None,
            diff:           sn_diff,
            diff_stat:      None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::test_support::FakeBackend;
    use crate::types::{Acceptance, Verdict};

    fn make_task() -> TaskSpec {
        TaskSpec {
            task_id:           "T-test".to_string(),
            spec_ref:          None,
            difficulty_bucket: None,
            prompt_path:       "/tmp/prompt.md".to_string(),
            project_path:      ".".to_string(),
            base_ref:          "HEAD".to_string(),
            acceptance:        Acceptance::ShellCommand {
                command: "true".to_string(),
            },
            synthetic:         false,
        }
    }

    #[test]
    fn runner_with_fake_backend_emits_rows() {
        let fake = FakeBackend::default();
        fake.program(Verdict::Pass, "canary: acceptance passed");
        let routes = two_tier_canary_routes("p0", "diff-mm".to_string(), "diff-sn".to_string());
        let task = make_task();
        let sink = std::env::temp_dir().join(format!("fabro-referee-test-{}", ulid::Ulid::new()));
        let res = run(&task, &routes, &fake, &sink, "run-test", None).unwrap();
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0].tier, "minimax");
        assert_eq!(res.rows[1].tier, "sonnet");
        // Both routes should have been scored.
        assert_eq!(fake.calls(), 2);
        // Verify verdicts are Pass (as programmed).
        assert!(res.rows.iter().all(|r| matches!(r.verdict, Verdict::Pass)));
    }

    #[test]
    fn make_row_propagates_synthetic_from_task_spec() {
        let mut task = make_task();
        task.synthetic = true;
        let route = &two_tier_canary_routes("p0", "d".into(), "d".into())[0];
        let gate = GateOutput {
            verdict:     Verdict::Pass,
            gate_log:    "ok".into(),
            backend:     "fake".into(),
            score:       Some(1.0),
            valset_hash: None,
        };
        let row = make_row("synthetic-x", &task, route, &gate);
        assert!(row.synthetic, "make_row MUST carry task.synthetic through to the row");
    }

    #[test]
    fn make_row_default_synthetic_is_false() {
        let task = make_task();
        let route = &two_tier_canary_routes("p0", "d".into(), "d".into())[0];
        let gate = GateOutput {
            verdict:     Verdict::Pass,
            gate_log:    "ok".into(),
            backend:     "fake".into(),
            score:       Some(1.0),
            valset_hash: None,
        };
        let row = make_row("real-x", &task, route, &gate);
        assert!(!row.synthetic);
    }
}
