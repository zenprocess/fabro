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
        use std::fmt::Write as _;
        use std::time::Duration;
        // Real forkd gate-run: spin a hermetic microVM from the golden snapshot,
        // run the task's closed-form acceptance INSIDE it, and map the exit code
        // to a verdict. Never fakes a pass: any setup failure (unreadable target,
        // failed seed, failed diff-apply) FAILS THE GATE CLOSED rather than
        // letting an unrelated pre-existing file state satisfy the acceptance.
        let token = std::env::var("FORKD_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let path = std::env::var("FORKD_TOKEN_FILE").unwrap_or_else(|_| {
                    format!(
                        "{}/fabro-run/.forkd-token",
                        std::env::var("HOME").unwrap_or_default()
                    )
                });
                std::fs::read_to_string(path)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            });
        let base = self.endpoint.clone();
        let req_timeout = Duration::from_secs(60);
        let auth = |rb: reqwest::blocking::RequestBuilder| {
            let rb = rb.timeout(req_timeout);
            match &token {
                Some(t) => rb.bearer_auth(t),
                None => rb,
            }
        };

        // 1. create a sandbox from the golden snapshot.
        let create = auth(self.client.post(format!("{base}/v1/sandboxes")))
            .json(&serde_json::json!({ "snapshot_tag": "zen-gate-base" }))
            .send()
            .with_context(|| format!("forkd create sandbox @ {base}"))?;
        let cstatus = create.status();
        let cbytes = create.bytes().context("read create-sandbox body")?;
        if !cstatus.is_success() {
            bail!(
                "forkd create sandbox non-2xx (status={cstatus}, body={})",
                String::from_utf8_lossy(&cbytes)
            );
        }
        let cjson: serde_json::Value = serde_json::from_slice(&cbytes).with_context(|| {
            format!(
                "create-sandbox invalid JSON: {}",
                String::from_utf8_lossy(&cbytes)
            )
        })?;
        let sid = cjson
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.get("id"))
            .or_else(|| cjson.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                // The VM may have been created but we cannot address it for
                // teardown without an id -> surface a possible orphan loudly.
                eprintln!(
                    "[forkd] WARNING: create-sandbox 2xx but no id in response; \
                     a sandbox may be orphaned: {}",
                    String::from_utf8_lossy(&cbytes)
                );
                anyhow!(
                    "create-sandbox: no sandbox id in {}",
                    String::from_utf8_lossy(&cbytes)
                )
            })?
            .to_string();

        // exec argv inside the sandbox -> (exit_code, stdout, stderr).
        let exec = |argv: Vec<String>| -> Result<(i64, String, String)> {
            let r = auth(self.client.post(format!("{base}/v1/sandboxes/{sid}/exec")))
                .json(&serde_json::json!({ "args": argv }))
                .send()
                .with_context(|| format!("forkd exec in sandbox {sid}"))?;
            let st = r.status();
            let b = r.bytes().context("read exec body")?;
            if !st.is_success() {
                bail!(
                    "forkd exec non-2xx (status={st}, body={})",
                    String::from_utf8_lossy(&b)
                );
            }
            let j: serde_json::Value = serde_json::from_slice(&b)
                .with_context(|| format!("exec invalid JSON: {}", String::from_utf8_lossy(&b)))?;
            Ok((
                j.get("exit_code")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(-1),
                j.get("stdout")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                j.get("stderr")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
            ))
        };

        let mut gate_log = String::new();
        // 2. run the acceptance; ? stays inside this closure so the sandbox is
        //    always torn down afterwards. Setup failures fail CLOSED.
        let outcome: Result<Verdict> = (|| {
            match &task.acceptance {
                crate::types::Acceptance::DiffMustMatch { pattern } => {
                    let re = regex::Regex::new(pattern)
                        .map_err(|e| anyhow!("bad diff_must_match regex {pattern}: {e}"))?;
                    let ok = re.is_match(route_diff);
                    let _ = writeln!(
                        gate_log,
                        "[forkd] diff_must_match /{pattern}/ -> {}",
                        if ok { "match" } else { "no-match" }
                    );
                    Ok(if ok { Verdict::Pass } else { Verdict::Fail })
                }
                crate::types::Acceptance::ShellCommand { command } => {
                    // Seed the acceptance target from the host IF it exists. An
                    // absent host file is fine -- the route diff may CREATE it --
                    // but a failed in-VM write fails CLOSED, and git-apply is
                    // enforced below so an unapplied diff can never pass on stale
                    // snapshot state. The path is single-quote-escaped like the
                    // content to avoid shell injection via prompt_path.
                    match std::fs::read_to_string(&task.prompt_path) {
                        Ok(content) => {
                            let esc = content.replace('\'', "'\\''");
                            let path_esc = task.prompt_path.replace('\'', "'\\''");
                            let (c, _o, e) = exec(vec![
                                "sh".into(),
                                "-c".into(),
                                format!(
                                    "mkdir -p \"$(dirname '{path_esc}')\" && printf '%s' '{esc}' > '{path_esc}'"
                                ),
                            ])?;
                            if c != 0 {
                                let _ = writeln!(
                                    gate_log,
                                    "[forkd] seed acceptance file FAILED exit={c}: {e}"
                                );
                                return Ok(Verdict::Fail);
                            }
                        }
                        Err(e) => {
                            let _ = writeln!(
                                gate_log,
                                "[forkd] acceptance target {} not seeded from host ({e}); \
                                 relying on route diff / snapshot",
                                task.prompt_path
                            );
                        }
                    }
                    // apply the route diff if present. If it does NOT apply, the
                    // acceptance would run against an unpatched tree -> fail closed.
                    if !route_diff.trim().is_empty() {
                        let dsc = route_diff.replace('\'', "'\\''");
                        let (dc, _do, de) = exec(vec![
                            "sh".into(),
                            "-c".into(),
                            format!("printf '%s' '{dsc}' | git apply --whitespace=nowarn"),
                        ])?;
                        if dc != 0 {
                            let _ = writeln!(gate_log, "[forkd] git apply FAILED exit={dc}: {de}");
                            return Ok(Verdict::Fail);
                        }
                        let _ = writeln!(gate_log, "[forkd] git apply ok");
                    }
                    // run the closed-form acceptance.
                    let (code, out, err) = exec(vec!["sh".into(), "-c".into(), command.clone()])?;
                    let _ = writeln!(gate_log, "[forkd] acceptance (exit={code}):\n{out}{err}");
                    Ok(if code == 0 { Verdict::Pass } else { Verdict::Fail })
                }
            }
        })();

        // 3. always delete the sandbox (best-effort); log teardown failure.
        if let Err(e) = auth(self.client.delete(format!("{base}/v1/sandboxes/{sid}"))).send() {
            eprintln!("[forkd] WARNING: sandbox {sid} delete failed (possible orphan): {e}");
        }

        let verdict = outcome?;
        Ok(GateOutput {
            verdict,
            gate_log,
            backend: "forkd".to_string(),
            score: None,
            valset_hash: None,
        })
    }

}

// ---------------------------------------------------------------------------
// HermeticLocal — contract-identical fallback
// ---------------------------------------------------------------------------

/// The hermetic local gate. Applies the diff in a throwaway git
/// checkout **seeded from `valset_root` at `base_ref`** (so the diff
/// is scored against the real repo content, not against an empty
/// tree), runs the task's closed-form acceptance, captures combined
/// stdout/stderr as `gate_log`, and returns `Pass` iff the acceptance
/// exited 0. Never fakes a pass.
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

/// Short-lived pipe thread: drain `reader` into `writer` in the
/// background. Both processes involved are local; the thread lives
/// only as long as the pipe. Declared at module scope (rather than
/// inline in `archive_seed`) so the items-after-statements lint
/// stays quiet and we can document the disallowed-method
/// expectation in one place.
#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: short-lived pipe thread is intentional; no Tokio runtime here"
)]
#[expect(
    clippy::disallowed_types,
    reason = "sync scorer binary: Read/Write trait bounds here; no Tokio runtime here"
)]
fn pump_between<R, W>(mut reader: R, mut writer: W) -> std::thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
    W: std::io::Write + Send + 'static,
{
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut reader, &mut writer);
    })
}

impl HermeticLocal {
    pub fn new(valset_root: PathBuf) -> Self {
        Self { valset_root }
    }

    /// Run `cmd` (consumed) and bail with stderr captured if it exits
    /// non-zero. Logs the invocation + verdict to `log`. Returns
    /// Ok(()) on success, Err on non-zero exit (which the caller is
    /// expected to convert into a `Verdict::Fail` row, never panic).
    ///
    /// `cmd` is mutated by `.output()` so we need `mut cmd: Command`.
    fn must_run(label: &str, log: &mut String, mut cmd: Command) -> std::io::Result<()> {
        let _ = writeln!(log, "[hermetic] {label}: {cmd:?}");
        let out = cmd.output()?;
        if !out.status.success() {
            let _ = writeln!(
                log,
                "[hermetic] {label} failed (status={}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr),
            );
            let status = out.status;
            let msg = format!("{label} exit={status}");
            return Err(std::io::Error::other(msg));
        }
        Ok(())
    }

    /// Build an owned `Command` (the chained builder methods return
    /// `&mut Command`, so we have to construct it inside a block to
    /// hand the caller the owned value).
    #[expect(
        clippy::disallowed_methods,
        reason = "sync scorer binary: std::process::Command is intentional; no Tokio runtime here"
    )]
    fn owned_cmd(program: &str) -> Command {
        Command::new(program)
    }

    /// Seed `workdir` from `valset_root` at `base_ref`, hermetically
    /// (no network). Prefers `git -C <valset_root> archive <base_ref> |
    /// tar -x -C <workdir>` when `valset_root` is itself a git repo —
    /// that pins the seed content to the exact tree at `base_ref`.
    /// Falls back to a recursive copy (skipping `valset_root/.git`)
    /// when no git history is available, so test fixtures that are
    /// plain directories still work.
    ///
    /// The default `base_ref == "HEAD"` resolves against the
    /// valset_root's git history; for the canary use case (valset_root
    /// == "." with a clean working tree at HEAD) both paths agree.
    fn seed_workdir(valset_root: &Path, base_ref: &str, workdir: &Path, log: &mut String) -> bool {
        let has_git = valset_root.join(".git").exists();
        if has_git {
            // Preferred path: git archive at base_ref piped to tar -x.
            // Both git archive and tar are local operations; no network.
            match Self::archive_seed(valset_root, base_ref, workdir) {
                Ok(()) => {
                    let _ = writeln!(
                        log,
                        "[hermetic] seeded workdir from {}@{} (git archive)",
                        valset_root.display(),
                        base_ref
                    );
                    return true;
                }
                Err(e) => {
                    let _ = writeln!(
                        log,
                        "[hermetic] git-archive seed failed: {e}; falling back to recursive copy"
                    );
                    // Fall through to the copy fallback below.
                }
            }
        }
        Self::copy_seed(valset_root, workdir, log)
    }

    /// `git -C <valset_root> archive <base_ref>` piped into
    /// `tar -x -C <workdir>`. Returns Err on any non-zero exit or
    /// spawn failure (with combined stderr captured).
    #[expect(
        clippy::disallowed_methods,
        reason = "sync scorer binary: git archive + tar subprocs are intentional; no Tokio runtime here"
    )]
    fn archive_seed(valset_root: &Path, base_ref: &str, workdir: &Path) -> std::io::Result<()> {
        let mut archive = Command::new("git")
            .arg("-C")
            .arg(valset_root)
            .args(["archive", base_ref])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut tar = Command::new("tar")
            .arg("-x")
            .arg("-C")
            .arg(workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        // Pump git archive's stdout → tar's stdin so we never buffer
        // a potentially-large archive entirely in memory. Use a
        // dedicated thread for the pump; both processes are local and
        // short-lived so the join is cheap.
        let archive_stdout = archive
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("git archive stdout"))?;
        let tar_stdin = tar
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("tar stdin"))?;
        let pump = pump_between(archive_stdout, tar_stdin);
        let archive_status = archive.wait()?;
        let _ = pump.join();
        let tar_status = tar.wait()?;
        if !archive_status.success() {
            return Err(std::io::Error::other(format!(
                "git archive exit={archive_status}"
            )));
        }
        if !tar_status.success() {
            return Err(std::io::Error::other(format!("tar exit={tar_status}")));
        }
        Ok(())
    }

    /// Recursive-copy fallback seeder. Walks `valset_root` and mirrors
    /// its contents into `workdir`, skipping `valset_root/.git` so we
    /// don't drag a parent repo history into the throwaway checkout.
    fn copy_seed(valset_root: &Path, workdir: &Path, log: &mut String) -> bool {
        #[expect(
            clippy::disallowed_methods,
            reason = "sync scorer binary: walk + copy + symlink are intentional; no Tokio runtime here"
        )]
        fn walk(src: &Path, dst: &Path) -> std::io::Result<()> {
            for entry in std::fs::read_dir(src)? {
                let entry = entry?;
                let ft = entry.file_type()?;
                let from = entry.path();
                let to = dst.join(entry.file_name());
                if ft.is_dir() {
                    if from.file_name().and_then(|s| s.to_str()) == Some(".git") {
                        continue;
                    }
                    std::fs::create_dir_all(&to)?;
                    walk(&from, &to)?;
                } else if ft.is_file() {
                    std::fs::copy(&from, &to)?;
                } else if ft.is_symlink() {
                    let target = std::fs::read_link(&from)?;
                    std::os::unix::fs::symlink(&target, &to).ok();
                }
            }
            Ok(())
        }
        match walk(valset_root, workdir) {
            Ok(()) => {
                let _ = writeln!(
                    log,
                    "[hermetic] seeded workdir from {} (recursive copy)",
                    valset_root.display()
                );
                true
            }
            Err(e) => {
                let _ = writeln!(log, "[hermetic] copy-seed failed: {e}");
                false
            }
        }
    }

    /// Apply `diff` in a throwaway checkout seeded from
    /// `valset_root` at `base_ref`. Returns `(applied, log)`. If the
    /// diff fails to apply (or the seed/seed-commit fails), returns
    /// `(false, log)` — the runner converts that into a `Verdict::Fail`
    /// row rather than an error, so the row is still emitted with the
    /// failure captured in `gate_log`.
    #[expect(
        clippy::disallowed_methods,
        reason = "sync scorer binary: git subprocesses via std::process::Command are intentional; no Tokio runtime here"
    )]
    fn apply_diff(
        task: &TaskSpec,
        diff: &str,
        valset_root: &Path,
        workdir: &Path,
    ) -> (bool, String) {
        let mut log = String::new();
        // Step 1: git init the throwaway workdir.
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
        // Both are now hard-required: non-zero exit is treated as a
        // hard failure (previously silently dropped). We use `-C
        // <workdir>` to scope the config to the throwaway checkout
        // (the original implementation passed the workdir as a
        // positional arg, which `git config` does not recognize and
        // silently fell back to the parent repo's config — that
        // surfaced here under sandbox-network-boundary enforcement
        // with a `lock config file ... Operation not permitted` error).
        let cfg_email = {
            let mut c = Self::owned_cmd("git");
            c.arg("-C").arg(workdir);
            c.args(["config", "user.email", "referee@local"]);
            c
        };
        if let Err(e) = Self::must_run("git config user.email", &mut log, cfg_email) {
            let _ = writeln!(log, "[hermetic] git config user.email failed: {e}");
            return (false, log);
        }
        let cfg_name = {
            let mut c = Self::owned_cmd("git");
            c.arg("-C").arg(workdir);
            c.args(["config", "user.name", "Referee"]);
            c
        };
        if let Err(e) = Self::must_run("git config user.name", &mut log, cfg_name) {
            let _ = writeln!(log, "[hermetic] git config user.name failed: {e}");
            return (false, log);
        }
        // Step 2: seed the workdir from valset_root at base_ref.
        if !Self::seed_workdir(valset_root, &task.base_ref, workdir, &mut log) {
            return (false, log);
        }
        // Commit the seeded tree as the base commit so `git apply` has
        // a real reference tree to apply the route diff against.
        // Non-zero exit (including "nothing to commit") is now a hard
        // failure with stderr captured.
        let add_out = Command::new("git")
            .arg("-C")
            .arg(workdir)
            .args(["add", "-A"])
            .output();
        let add_out = match add_out {
            Ok(o) => o,
            Err(e) => {
                let _ = writeln!(log, "[hermetic] git add spawn failed: {e}");
                return (false, log);
            }
        };
        if !add_out.status.success() {
            let _ = writeln!(
                log,
                "[hermetic] git add failed (status={}): {}",
                add_out.status,
                String::from_utf8_lossy(&add_out.stderr),
            );
            return (false, log);
        }
        let commit_out = Command::new("git")
            .arg("-C")
            .arg(workdir)
            .args([
                "commit",
                "-q",
                "-m",
                "valset (seed from valset_root@base_ref)",
                "--allow-empty",
            ])
            .output();
        let commit_out = match commit_out {
            Ok(o) => o,
            Err(e) => {
                let _ = writeln!(log, "[hermetic] git commit spawn failed: {e}");
                return (false, log);
            }
        };
        if !commit_out.status.success() {
            let _ = writeln!(
                log,
                "[hermetic] git commit failed (status={}): {}",
                commit_out.status,
                String::from_utf8_lossy(&commit_out.stderr),
            );
            return (false, log);
        }
        // Step 3: apply the route diff on top of the seeded base.
        // git apply with --3way is forgiving about whitespace.
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
        {
            let Some(stdin) = child.stdin.as_mut() else {
                let _ = writeln!(log, "[hermetic] git apply child stdin unavailable");
                return (false, log);
            };
            if let Err(e) = stdin.write_all(diff.as_bytes()) {
                let _ = writeln!(log, "[hermetic] write diff stdin failed: {e}");
                return (false, log);
            }
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
        reason = "sync scorer binary: acceptance subprocess via std::process::Command is intentional; no Tokio runtime here"
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
            "[hermetic] begin task={} diff_bytes={} valset_root={} base_ref={}",
            task.task_id,
            route_diff.len(),
            self.valset_root.display(),
            task.base_ref,
        );
        let (applied, apply_log) = Self::apply_diff(task, route_diff, &self.valset_root, &tmp);
        gate_log.push_str(&apply_log);
        if !applied {
            // Drop the tempdir. Best-effort.
            let _ = std::fs::remove_dir_all(&tmp);
            gate_log.push_str(
                "[hermetic] verdict=FAIL (diff did not apply against seeded valset_root)\n",
            );
            info!(task = %task.task_id, "hermetic: diff did not apply");
            // valsethash is ONLY recorded after a successful apply+seed:
            // a FAIL row carries the failure reason in gate_log with no
            // misleading hash (the hash would imply the diff had been
            // scored against the real repo when in fact it never reached
            // the acceptance step).
            return Ok(GateOutput {
                verdict: Verdict::Fail,
                gate_log,
                backend: "hermetic".to_string(),
                score: Some(0.0),
                valset_hash: None,
            });
        }
        let (exit_code, accept_log) = Self::run_acceptance(task, route_diff, &tmp);
        gate_log.push_str(&accept_log);
        let _ = std::fs::remove_dir_all(&tmp);
        let verdict = if exit_code == 0 {
            Verdict::Pass
            // valset_hash now lives on the Pass AND the Fail-from-acceptance
            // rows — both happen after a successful seed+apply, so the
            // hash reflects what was actually scored.
        } else {
            Verdict::Fail
        };
        let valset_hash = Some(Self::compute_valset_hash(task, route_diff));
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
            valset_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::Acceptance;

    /// Synchronous file write for test fixtures. Lives at module
    /// scope so the disallowed-methods lint only needs to be silenced
    /// in one place, and to dodge the "items after statements"
    /// lint that forbids defining helpers inline after `let`.
    #[expect(
        clippy::disallowed_methods,
        reason = "synchronous test fixture write; no Tokio runtime here"
    )]
    fn write_file(path: &std::path::Path, contents: &str) {
        std::fs::write(path, contents).unwrap();
    }

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

    /// SAFETY-CRITICAL regression: a diff that *modifies* an existing
    /// file in `valset_root` must apply and score correctly. Before the
    /// fix, the throwaway workdir was seeded from EMPTY, so any
    /// modify-existing-file diff failed to apply (or, with `--3way`,
    /// silently miscompared against an absent base) — a Pass then meant
    /// only "git could synthesize a new file", not "the diff solved
    /// the task against the real repo".
    #[test]
    fn hermetic_local_modifies_existing_file_against_seeded_valset_root() {
        // Build a one-file valset in a unique tempdir. We seed the
        // HermeticLocal against THIS directory (not the current
        // worktree) so the test is hermetic and self-contained.
        let valset_root =
            std::env::temp_dir().join(format!("fabro-referee-valset-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&valset_root).unwrap();
        let existing_file = "EXISTING.md";
        let existing_contents = "line one\nline two\nline three\n";
        write_file(&valset_root.join(existing_file), existing_contents);

        // A diff that MODIFIES the existing file: changes line two's
        // content. Single-line format string (no `\` continuation,
        // which strips leading whitespace and silently invalidates
        // the leading-space context markers git apply requires).
        let modified_diff = format!(
            "--- a/{existing_file}\n+++ b/{existing_file}\n@@ -1,3 +1,3 @@\n line one\n-line two\n+LINE TWO MODIFIED\n line three\n"
        );

        // Acceptance: only pass if line TWO carries the marker text.
        // The acceptance runs in the *seeded* workdir (which holds
        // EXISTING.md after the apply succeeded), proving the diff
        // landed against the REAL tree, not an empty one.
        let task_pass = TaskSpec {
            task_id:           "T-modify-existing".to_string(),
            spec_ref:          None,
            difficulty_bucket: None,
            prompt_path:       valset_root
                .join(existing_file)
                .to_string_lossy()
                .to_string(),
            project_path:      valset_root.to_string_lossy().to_string(),
            base_ref:          "HEAD".to_string(),
            acceptance:        Acceptance::ShellCommand {
                command: format!("grep -F 'LINE TWO MODIFIED' {existing_file}"),
            },
        };
        let backend = HermeticLocal::new(valset_root.clone());
        let out = backend.score(&task_pass, &modified_diff).unwrap();
        assert_eq!(
            out.verdict,
            Verdict::Pass,
            "modify-existing-file diff against seeded valset_root MUST pass; log={}",
            out.gate_log
        );
        assert!(
            out.gate_log.contains("git apply ok"),
            "gate_log must show the apply step succeeded; got={}",
            out.gate_log
        );
        assert!(
            out.valset_hash.is_some(),
            "valset_hash must be recorded on a Pass row (applied cleanly)"
        );

        // Negative path: a diff that does NOT carry the marker in
        // the seeded file must FAIL the acceptance (grep exit 1).
        // This proves the seed was real: if the workdir were empty,
        // applying a modify-existing-file diff could not have
        // produced a tree to grep against in the first place.
        let wrong_diff = format!(
            "--- a/{existing_file}\n+++ b/{existing_file}\n@@ -1,3 +1,3 @@\n line one\n-line two\n+something else entirely\n line three\n"
        );
        let out_fail = backend.score(&task_pass, &wrong_diff).unwrap();
        assert_eq!(
            out_fail.verdict,
            Verdict::Fail,
            "diff against the seeded file that does NOT add the marker MUST fail"
        );
        assert!(
            out_fail.valset_hash.is_some(),
            "valset_hash IS recorded after successful seed+apply; only the diff-did-not-apply branch omits it"
        );

        // Cleanup.
        let _ = std::fs::remove_dir_all(&valset_root);
    }
}
