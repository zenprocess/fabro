//! Ignored live smoke for the authenticated forkd sandbox gate.
//!
//! This test is deliberately operator-only. It is ignored and also returns
//! without touching the network unless FORKD_TOKEN is present.

use fabro_referee::gate::GateBackend;
use fabro_referee::gate::backend::ForkdController;
use fabro_referee::types::{Acceptance, TaskSpec, Verdict};

#[test]
#[ignore = "live forkd controller; run explicitly from an egress-allowlisted host"]
#[expect(
    clippy::disallowed_methods,
    clippy::print_stderr,
    reason = "operator-only live smoke: documented process-env lookup facade to gate on FORKD_TOKEN/FORKD_ENDPOINT presence; eprintln reports the skip when run explicitly"
)]
fn live_forkd_score_smoke() {
    if std::env::var_os("FORKD_TOKEN").is_none() {
        eprintln!("FORKD_TOKEN is unset; skipping live forkd smoke");
        return;
    }

    let endpoint =
        std::env::var("FORKD_ENDPOINT").unwrap_or_else(|_| "http://dellsrv:8891".to_string());
    let controller = ForkdController::new(&endpoint).expect("build authenticated forkd client");
    let task = TaskSpec {
        task_id:           "T-live-forkd-score-smoke".to_string(),
        spec_ref:          None,
        difficulty_bucket: Some("easy".to_string()),
        prompt_path:       "CANARY.md".to_string(),
        project_path:      ".".to_string(),
        base_ref:          "HEAD".to_string(),
        acceptance:        Acceptance::ShellCommand {
            command: "grep -F 'P0-CANARY:' CANARY.md".to_string(),
        },
        synthetic:         false,
    };
    let diff = "--- a/CANARY.md\n+++ b/CANARY.md\n@@ -0,0 +1,1 @@\n+P0-CANARY: live-forkd-smoke\n";

    let output = controller
        .score(&task, diff)
        .expect("live forkd create/exec/delete flow should succeed");
    assert_eq!(output.backend, "forkd");
    assert_eq!(
        output.verdict,
        Verdict::Pass,
        "gate log: {}",
        output.gate_log
    );
    assert!(!output.gate_log.is_empty());
}
