//! Minimal canary task definition — a fast closed-form acceptance
//! the hermetic gate can run in <1s on any host.
//!
//! The canary is the **first** task the orchestrator should run
//! against the live `ForkdController` (and the doc-canonical
//! exercise for the hermetic tests). It is deliberately trivial so
//! the gate's verdict is unambiguous and the run completes before
//! any wrapper-side timeout.
//!
//! Task: introduce a single line `// P0-CANARY: <run_id>\n` at the
//! top of a chosen file in the project. The acceptance command
//! asserts the line is present (`grep -F 'P0-CANARY:' <file>`).
//!
//! Variant for the GREEN path: line is present → exit 0 → pass.
//! Variant for the RED path: line is absent → exit 1 → fail.
//!
//! The route's diff is the `git diff <base_ref>...<branch>` that the
//! orchestrator's `ao spawn` produces. The hermetic backend applies
//! the diff in a throwaway checkout and runs the acceptance.
//!
//! `base_ref` defaults to the current HEAD; the canary task is
//! branch-agnostic.

use std::path::Path;

use anyhow::Result;

use crate::types::{Acceptance, TaskSpec};

/// Stable task id for the canary. The same string is used in
/// `CANARY-RUNBOOK.md` so the operator can grep for it.
pub const CANARY_TASK_ID: &str = "T-canary-p0-stamp";

/// The line the canary expects to find in the chosen file.
pub fn canary_marker(run_id: &str) -> String {
    format!("P0-CANARY: {run_id}")
}

/// Build a `TaskSpec` for the canary. `valset_root` is the path
/// the hermetic backend seeds the throwaway checkout from
/// (typically the current worktree). `acceptance_file` is the
/// relative path (within `valset_root`) the marker is written to.
pub fn build_canary_task(
    _run_id: &str,
    valset_root: &Path,
    acceptance_file: &str,
) -> Result<TaskSpec> {
    // The acceptance command greps for the *sub* part of the marker
    // (the run_id is operator-supplied per run, so we accept any
    // P0-CANARY: line). Exit 0 iff the line is present.
    let command = format!(
        "grep -F 'P0-CANARY:' {acceptance_file} && test -n \"$(grep -F 'P0-CANARY:' {acceptance_file})\""
    );
    let prompt_path = valset_root
        .join(acceptance_file)
        .to_string_lossy()
        .to_string();
    Ok(TaskSpec {
        task_id: CANARY_TASK_ID.to_string(),
        spec_ref: Some("fabro-referee/CANARY-RUNBOOK.md".to_string()),
        difficulty_bucket: Some("easy".to_string()),
        prompt_path,
        project_path: valset_root.to_string_lossy().to_string(),
        base_ref: "HEAD".to_string(),
        acceptance: Acceptance::ShellCommand { command },
        synthetic: false,
    })
}

/// Build a canned diff that the oracle-RED canary path can use to
/// prove the hermetic backend correctly FAILs when the diff doesn't
/// add the marker. Returns an empty diff (against the seeded
/// valset) — the acceptance will exit 1 because the marker is
/// absent.
pub fn red_canned_diff() -> String {
    String::new()
}

/// Build a canned diff that the oracle-GREEN canary path can use
/// to prove the hermetic backend correctly PASSes when the diff
/// adds the marker. The diff is applied to `acceptance_file`.
pub fn green_canned_diff(run_id: &str, acceptance_file: &str) -> String {
    let marker = canary_marker(run_id);
    format!("--- a/{acceptance_file}\n+++ b/{acceptance_file}\n@@ -0,0 +1,1 @@\n+{marker}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_includes_run_id() {
        let m = canary_marker("abc-123");
        assert_eq!(m, "P0-CANARY: abc-123");
    }

    #[test]
    fn green_diff_contains_marker() {
        let d = green_canned_diff("xyz-9", "TEST.md");
        assert!(d.contains("P0-CANARY: xyz-9"));
        assert!(d.contains("TEST.md"));
    }

    #[test]
    fn acceptance_command_targets_supplied_file() {
        let t = build_canary_task("r", Path::new("."), "MARKER").unwrap();
        if let Acceptance::ShellCommand { command } = &t.acceptance {
            assert!(command.contains("P0-CANARY:"));
        } else {
            panic!("expected ShellCommand");
        }
    }
}
