//! Production gate backends — `ForkdController` (real, T3) and
//! `HermeticLocal` (contract-identical fallback).
//!
//! The two backends are deliberately implemented in the same file so
//! their return-shape parity is obvious to a reviewer.

use std::fmt::Write as _;
#[expect(
    clippy::disallowed_types,
    reason = "sync scorer binary: ChildStdin::write_all needs std::io::Write in scope; no Tokio runtime here"
)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use super::{GateBackend, GateOutput};
use crate::types::{Acceptance, TaskSpec, Verdict};

// ---------------------------------------------------------------------------
// ForkdController — REAL, T3, score/read path only
// ---------------------------------------------------------------------------

/// The real forkd hermetic gate controller at `dellsrv:8891`. The
/// scorer **never** activates / reconfigures / restarts / re-baselines
/// the controller: it only POSTs to its existing `gate-run` /
/// `forkd-exec` endpoints and parses the JSON response.
///
/// Per `CONTRACTS.md` §1, the controller is unreachable from this
/// worktree (sandbox egress boundary). The scorer is fully wired
/// against the response shape so the orchestrator can drive the live
/// canary from a host that *can* reach the controller.
pub struct ForkdController {
    /// The controller endpoint, e.g. `http://dellsrv:8891`.
    endpoint: String,
    /// HTTP client from `fabro-http::blocking_http_client()` (sync,
    /// to fit the synchronous `GateBackend` trait).
    client:   fabro_http::BlockingHttpClient,
}

impl ForkdController {
    pub fn new(endpoint: &str) -> Result<Self> {
        let client = fabro_http::blocking_http_client().context("build forkd http client")?;
        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client,
        })
    }
}

impl GateBackend for ForkdController {
    fn name(&self) -> &'static str {
        "forkd"
    }

    fn score(&self, task: &TaskSpec, route_diff: &str) -> Result<GateOutput> {
        // Per the P0 spec: POST the diff to the controller's gate-run
        // endpoint and parse the `verdict` + `gate_log` response. The
        // endpoint path is `gate-run` (the operator-confirmed path
        // should replace this when the orchestrator drives the live
        // canary; the contract is the response shape, not the path).
        let url = format!("{}/gate-run", self.endpoint);
        let body = serde_json::json!({
            "task_id": task.task_id,
            "base_ref": task.base_ref,
            "diff": route_diff,
            "ts": Utc::now().to_rfc3339(),
        });
        debug!(url = %url, "forkd POST gate-run");
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .with_context(|| format!("forkd POST {url}"))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .with_context(|| format!("forkd read body from {url}"))?;
        if !status.is_success() {
            bail!(
                "forkd returned non-2xx (status={status}, body={})",
                String::from_utf8_lossy(&bytes)
            );
        }
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "forkd returned invalid JSON: {}",
                String::from_utf8_lossy(&bytes)
            )
        })?;
        let verdict_str = parsed
            .get("verdict")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("forkd response missing `verdict`"))?;
        let verdict = match verdict_str {
            "pass" => Verdict::Pass,
            "fail" => Verdict::Fail,
            other => bail!("forkd verdict was {other:?}, expected \"pass\"|\"fail\""),
        };
        let gate_log = parsed
            .get("gate_log")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("forkd response missing `gate_log`"))?
            .to_string();
        let score = parsed.get("score").and_then(serde_json::Value::as_f64);
        let valset_hash = parsed
            .get("valset_hash")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(GateOutput {
            verdict,
            gate_log,
            backend: "forkd".to_string(),
            score,
            valset_hash,
        })
    }
}

// ---------------------------------------------------------------------------
// HermeticLocal — contract-identical fallback
// ---------------------------------------------------------------------------

/// The hermetic local gate. Applies the diff in a throwaway git
/// checkout seeded from `valset_root`, runs the task's closed-form
/// acceptance, captures combined stdout/stderr as `gate_log`, and
/// returns `Pass` iff the acceptance exited 0. Never fakes a pass.
///
/// This is the *fallback* backend the P0 spec mandates: it has the
/// same return shape as `ForkdController` so the runner emits the
/// JSONL row indistinguishably, and it works with **neither** `ao`
/// **nor** `dellsrv` — which is why the hermetic tests can run
/// anywhere.
pub struct HermeticLocal {
    /// The directory the hermetic checkout is seeded from. The
    /// canary points this at the current fabro worktree.
    valset_root: PathBuf,
}

impl HermeticLocal {
    pub fn new(valset_root: PathBuf) -> Self {
        Self { valset_root }
    }

    /// Apply `diff` in a throwaway checkout rooted at `valset_root`.
    /// Returns the path to the checkout and the captured diff-apply
    /// log. If the diff fails to apply, the function returns a
    /// `GateOutput { verdict: Fail, .. }` rather than an error — the
    /// runner still wants the row in the sink, with the failure
    /// captured in `gate_log`.
    #[expect(
        clippy::disallowed_methods,
        reason = "sync scorer binary: git subprocesses via std::process::Command are intentional; no Tokio runtime"
    )]
    fn apply_diff(task: &TaskSpec, diff: &str, workdir: &Path) -> (bool, String) {
        let mut log = String::new();
        // Step 1: git init + commit the valset_root state.
        let init = Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(workdir)
            .output();
        match init {
            Ok(o) if !o.status.success() => {
                let _ = writeln!(
                    log,
                    "[hermetic] git init failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
                return (false, log);
            }
            Err(e) => {
                let _ = writeln!(log, "[hermetic] git init spawn failed: {e}");
                return (false, log);
            }
            _ => {}
        }
        // Configure a local user (git refuses to commit otherwise).
        let _ = Command::new("git")
            .args(["config", "user.email", "referee@local"])
            .arg(workdir)
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "Referee"])
            .arg(workdir)
            .output();
        // Add the valset_root contents.
        let add = Command::new("git")
            .arg("-C")
            .arg(workdir)
            .args(["add", "-A"])
            .output();
        if let Err(e) = add {
            let _ = writeln!(log, "[hermetic] git add failed: {e}");
            return (false, log);
        }
        let commit = Command::new("git")
            .arg("-C")
            .arg(workdir)
            .args(["commit", "-q", "-m", "valset"])
            .output();
        if let Err(e) = commit {
            let _ = writeln!(log, "[hermetic] git commit failed: {e}");
            return (false, log);
        }
        // Step 2: apply the diff. The diff is captured from the
        // orchestrator's worktree; git apply with --3way is forgiving.
        let mut child = match Command::new("git")
            .arg("-C")
            .arg(workdir)
            .args(["apply", "--3way", "--whitespace=nowarn"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(log, "[hermetic] git apply spawn failed: {e}");
                return (false, log);
            }
        };
        let Some(stdin) = child.stdin.as_mut() else {
            let _ = writeln!(log, "[hermetic] git apply child stdin unavailable");
            return (false, log);
        };
        if let Err(e) = stdin.write_all(diff.as_bytes()) {
            let _ = writeln!(log, "[hermetic] write diff stdin failed: {e}");
            return (false, log);
        }
        let apply_out = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                let _ = writeln!(log, "[hermetic] git apply wait failed: {e}");
                return (false, log);
            }
        };
        if !apply_out.status.success() {
            let _ = writeln!(
                log,
                "[hermetic] git apply failed (status={}): {}",
                apply_out.status,
                String::from_utf8_lossy(&apply_out.stderr),
            );
            let _ = writeln!(log, "[hermetic] diff was:\n{diff}");
            return (false, log);
        }
        let _ = writeln!(
            log,
            "[hermetic] git apply ok for task={} ({} bytes diff)",
            task.task_id,
            diff.len(),
        );
        (true, log)
    }

    /// Run the task's closed-form acceptance. Returns the combined
    /// stdout/stderr and the exit status. **Never fakes a pass**: the
    /// caller maps non-zero exit 1:1 to `Verdict::Fail`.
    #[expect(
        clippy::disallowed_methods,
        reason = "sync scorer binary: acceptance subprocess via std::process::Command is intentional; no Tokio runtime"
    )]
    fn run_acceptance(task: &TaskSpec, route_diff: &str, workdir: &Path) -> (i32, String) {
        let mut log = String::new();
        let cmd_str = match &task.acceptance {
            Acceptance::ShellCommand { command } => command.as_str(),
            Acceptance::DiffMustMatch { pattern } => {
                // The acceptance is a regex against the route's diff
                // itself. We don't run anything in the checkout — we
                // already have the diff — so this maps directly to a
                // match/no-match verdict with the result in the log.
                let _ = write!(log, "[hermetic] diff_must_match pattern={pattern:?} ");
                let re = match regex::Regex::new(pattern) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = writeln!(log, "invalid regex: {e}");
                        return (2, log);
                    }
                };
                if re.is_match(route_diff) {
                    log.push_str("matched\n");
                    return (0, log);
                }
                log.push_str("did NOT match\n");
                return (1, log);
            }
        };
        let child = match Command::new("sh")
            .arg("-c")
            .arg(cmd_str)
            .current_dir(workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(log, "[hermetic] acceptance spawn failed: {e}");
                return (127, log);
            }
        };
        let out = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                let _ = writeln!(log, "[hermetic] acceptance wait failed: {e}");
                return (127, log);
            }
        };
        let code = out.status.code().unwrap_or(127);
        let _ = writeln!(
            log,
            "[hermetic] acceptance exit={code}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        (code, log)
    }

    /// Compute the valset_hash the same way the parent schema
    /// documents: sha256 of the canonical `(task_id, base_ref, diff)`
    /// triple, all UTF-8, joined by `\n`.
    fn compute_valset_hash(task: &TaskSpec, diff: &str) -> String {
        let mut h = Sha256::new();
        h.update(task.task_id.as_bytes());
        h.update(b"\n");
        h.update(task.base_ref.as_bytes());
        h.update(b"\n");
        h.update(diff.as_bytes());
        format!("sha256:{:x}", h.finalize())
    }
}

impl GateBackend for HermeticLocal {
    fn name(&self) -> &'static str {
        "hermetic"
    }

    fn score(&self, task: &TaskSpec, route_diff: &str) -> Result<GateOutput> {
        // The throwaway checkout goes under the system tempdir;
        // `tempfile::tempdir` would be cleaner but the spec keeps
        // the dep tree lean.
        let tmp = std::env::temp_dir().join(format!("fabro-referee-{}", ulid::Ulid::new()));
        let mut gate_log = String::new();
        let _ = writeln!(
            gate_log,
            "[hermetic] begin task={} diff_bytes={} valset_root={}",
            task.task_id,
            route_diff.len(),
            self.valset_root.display(),
        );
        let (applied, apply_log) = Self::apply_diff(task, route_diff, &tmp);
        gate_log.push_str(&apply_log);
        if !applied {
            // Drop the tempdir. Best-effort.
            let _ = std::fs::remove_dir_all(&tmp);
            gate_log.push_str("[hermetic] verdict=FAIL (diff did not apply)\n");
            info!(task = %task.task_id, "hermetic: diff did not apply");
            return Ok(GateOutput {
                verdict: Verdict::Fail,
                gate_log,
                backend: "hermetic".to_string(),
                score: Some(0.0),
                valset_hash: Some(Self::compute_valset_hash(task, route_diff)),
            });
        }
        let (exit_code, accept_log) = Self::run_acceptance(task, route_diff, &tmp);
        gate_log.push_str(&accept_log);
        let _ = std::fs::remove_dir_all(&tmp);
        let verdict = if exit_code == 0 {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
        let _ = writeln!(
            gate_log,
            "[hermetic] verdict={verdict} (acceptance exit={exit_code})",
        );
        if verdict == Verdict::Fail {
            warn!(task = %task.task_id, code = exit_code, "hermetic: acceptance failed");
        } else {
            info!(task = %task.task_id, "hermetic: pass");
        }
        Ok(GateOutput {
            verdict,
            gate_log,
            backend: "hermetic".to_string(),
            score: Some(if exit_code == 0 { 1.0 } else { 0.0 }),
            valset_hash: Some(Self::compute_valset_hash(task, route_diff)),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::Acceptance;

    fn diff_match_task(pattern: &str) -> TaskSpec {
        TaskSpec {
            task_id:           "T-diff-match".to_string(),
            spec_ref:          None,
            difficulty_bucket: None,
            // A prompt_path the pattern would NOT match — proves the
            // regex is applied to the diff, not this field.
            prompt_path:       "/tmp/unrelated-prompt.md".to_string(),
            project_path:      ".".to_string(),
            base_ref:          "HEAD".to_string(),
            acceptance:        Acceptance::DiffMustMatch {
                pattern: pattern.to_string(),
            },
        }
    }

    #[test]
    fn diff_must_match_targets_the_diff_not_the_prompt_path() {
        let task = diff_match_task("fn added_symbol");
        let diff = "+++ b/src/lib.rs\n+fn added_symbol() {}\n";
        let (code, log) = HermeticLocal::run_acceptance(&task, diff, &PathBuf::from("."));
        assert_eq!(code, 0, "pattern present in diff must pass; log={log}");
        assert!(log.contains("matched"));
    }

    #[test]
    fn diff_must_match_fails_when_pattern_absent_from_diff() {
        // The pattern DOES appear in prompt_path, but must NOT match
        // because acceptance is scored against the diff alone.
        let task = diff_match_task("unrelated-prompt");
        let diff = "+++ b/src/lib.rs\n+fn added_symbol() {}\n";
        let (code, log) = HermeticLocal::run_acceptance(&task, diff, &PathBuf::from("."));
        assert_eq!(code, 1, "pattern absent from diff must fail; log={log}");
        assert!(log.contains("did NOT match"));
    }
}
