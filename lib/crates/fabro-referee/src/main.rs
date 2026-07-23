//! `fabro-referee` — the P0 per-attempt, per-tier scorer CLI.
//!
//! Operational subcommands:
//!
//! * `score` — score one task × N tiers against the chosen backend and emit the
//!   JSONL + .md summary. The `routes` and `diffs` are supplied as a JSON file
//!   (when the orchestrator captured the diffs in advance) or via `--from-git`
//!   (when the runner captures them on the fly from `git -C <project> diff
//!   <base>...<branch>`).
//!
//! * `canary` — convenience wrapper that builds the canary task
//!   (`canary::build_canary_task`) and runs `score` against two routes:
//!   `<branch>-mm` and `<branch>-sn`. Intended for the orchestrator's first
//!   live canary after this P0 lands.
//!
//! * `doctor` — print the resolved contracts (forkd endpoint reach,
//!   decision-log path, sink dir) without scoring anything. Used by the canary
//!   runbook to triage a "no row in the sink" investigation before poking the
//!   orchestrator.
//!
//! All subcommands honor `--backend hermetic` (default) or
//! `--backend forkd`. `--backend forkd` requires the egress
//! allowlist to include `dellsrv:8891` (it does not from this
//! worktree).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fabro_referee::canary::{
    CANARY_TASK_ID, build_canary_task, canary_marker, green_canned_diff, red_canned_diff,
};
use fabro_referee::emit::default_sink_dir;
use fabro_referee::gate::backend::forkd_token;
use fabro_referee::gate::{BackendKind, GateBackend};
use fabro_referee::runner::{run, two_tier_canary_routes};
use fabro_referee::types::{Route, TaskSpec};

#[derive(Parser, Debug)]
#[command(
    name = "fabro-referee",
    about = "AO Factory Referee plane (P0)",
    version
)]
struct Cli {
    /// Path to the wrapper-decisions.jsonl (default:
    /// ~/.ao/data/wrapper-decisions.jsonl).
    #[arg(long, global = true)]
    decision_log: Option<PathBuf>,

    /// Sink directory for .jsonl + .md output (default:
    /// ~/.ao/data/aofactory/referee/runs).
    #[arg(long, global = true)]
    sink_dir: Option<PathBuf>,

    /// Backend kind: hermetic (default, no network) | forkd (real,
    /// needs dellsrv:8891 in the egress allowlist) | fake (test-only).
    #[arg(long, global = true, default_value = "hermetic")]
    backend: String,

    /// Forkd controller endpoint (only used by `--backend forkd`).
    #[arg(
        long,
        global = true,
        default_value = "http://dellsrv:8891",
        value_name = "URL"
    )]
    forkd_endpoint: String,

    /// Path the hermetic backend seeds the throwaway checkout from.
    #[arg(long, global = true, default_value = ".")]
    valset_root: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Score one task × N tiers against the chosen backend.
    Score {
        /// Path to the task spec JSON file.
        #[arg(long)]
        task:   PathBuf,
        /// Path to the routes JSON file (Vec<Route> serialized).
        #[arg(long)]
        routes: PathBuf,
        /// Run id (one canary = one run_id).
        #[arg(long)]
        run_id: String,
    },
    /// Score the canary task against two routes (mm + sn).
    Canary {
        /// Run id (one canary = one run_id).
        #[arg(long)]
        run_id:      String,
        /// Base branch name (the orchestrator's prefix; the runner
        /// appends `-mm` / `-sn`).
        #[arg(long, default_value = "p0-canary")]
        branch_base: String,
        /// Relative path the canary marker is added to.
        #[arg(long, default_value = "CANARY.md")]
        file:        String,
        /// Inject the GREEN canned diff (the marker line) for both
        /// routes; otherwise both routes use the RED canned diff
        /// (no marker) so the hermetic gate correctly FAILs.
        #[arg(long, default_value_t = false)]
        green:       bool,
    },
    /// Print the resolved contracts; do not score.
    Doctor,
}

#[expect(
    clippy::print_stderr,
    reason = "CLI binary: operator-facing run status is written to stderr"
)]
fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let sink_dir = cli.sink_dir.clone().unwrap_or_else(default_sink_dir);
    let decision_log = cli.decision_log.clone().or_else(default_decision_log_path);
    let backend_kind = BackendKind::from_str_loose(&cli.backend)
        .with_context(|| format!("unknown backend `{}`", cli.backend))?;
    let backend: Box<dyn GateBackend> =
        backend_kind
            .build(&cli.forkd_endpoint, &cli.valset_root)
            .with_context(|| format!("build backend `{}`", cli.backend))?;

    match cli.command {
        Command::Score {
            task,
            routes,
            run_id,
        } => {
            let task = load_task(&task)?;
            let routes = load_routes(&routes)?;
            let result = run(
                &task,
                &routes,
                backend.as_ref(),
                &sink_dir,
                &run_id,
                decision_log.as_deref(),
            )?;
            eprintln!(
                "fabro-referee: run_id={} rows={} sink_dir={}",
                result.run_id,
                result.rows.len(),
                sink_dir.display(),
            );
            for r in &result.rows {
                eprintln!(
                    "  {:>3} tier={:<7} verdict={:<4} backend={}",
                    r.route, r.tier, r.verdict, r.gate_backend
                );
            }
        }
        Command::Canary {
            run_id,
            branch_base,
            file,
            green,
        } => {
            let task = build_canary_task(&run_id, &cli.valset_root, &file)?;
            // Two routes: the orchestrator's `<base>-mm` and
            // `<base>-sn`. The diffs are canned at the CLI layer —
            // when the orchestrator drives the live canary it
            // replaces these with the real `git diff` output.
            let (mm_diff, sn_diff) = if green {
                (
                    green_canned_diff(&run_id, &file),
                    green_canned_diff(&run_id, &file),
                )
            } else {
                (red_canned_diff(), red_canned_diff())
            };
            let routes = two_tier_canary_routes(&branch_base, mm_diff, sn_diff);
            let result = run(
                &task,
                &routes,
                backend.as_ref(),
                &sink_dir,
                &run_id,
                decision_log.as_deref(),
            )?;
            eprintln!(
                "fabro-referee canary: run_id={} task={} marker={:?}",
                result.run_id,
                task.task_id,
                canary_marker(&run_id),
            );
            for r in &result.rows {
                eprintln!(
                    "  {:>3} branch={} verdict={:<4} backend={}",
                    r.route, r.branch, r.verdict, r.gate_backend
                );
            }
        }
        Command::Doctor => {
            do_doctor(&cli, &sink_dir, decision_log.as_deref(), backend_kind);
        }
    }
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: one-shot read of a small task-spec JSON; no Tokio runtime here"
)]
fn load_task(path: &Path) -> Result<TaskSpec> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read task spec {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse task spec {}", path.display()))
}

#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: one-shot read of a small routes JSON; no Tokio runtime here"
)]
fn load_routes(path: &Path) -> Result<Vec<Route>> {
    let bytes = std::fs::read(path).with_context(|| format!("read routes {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse routes {}", path.display()))
}

#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: one-shot HOME lookup to locate the decision log; no Tokio runtime here"
)]
fn default_decision_log_path() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var("HOME").ok()?);
    p.push(".ao");
    p.push("data");
    p.push("wrapper-decisions.jsonl");
    if p.exists() { Some(p) } else { None }
}

#[expect(
    clippy::print_stdout,
    reason = "CLI binary: `doctor` prints the resolved contracts to stdout by design"
)]
fn do_doctor(cli: &Cli, sink_dir: &Path, decision_log: Option<&Path>, backend_kind: BackendKind) {
    println!("fabro-referee doctor");
    println!(
        "  backend       : {:?} (endpoint={})",
        backend_kind, cli.forkd_endpoint
    );
    println!("  sink_dir      : {}", sink_dir.display());
    println!("  valset_root   : {}", cli.valset_root.display());
    println!(
        "  decision_log  : {}",
        decision_log.map_or_else(|| "<absent>".to_string(), |p| p.display().to_string())
    );
    println!("  task_id default: {CANARY_TASK_ID}");
    println!("  canary file  : CANARY.md (operator-overridable via --file)");
    // Cheap reachability check for the forkd endpoint (one sync HTTP
    // GET). We do NOT fail if dellsrv is unreachable — that is the
    // egress boundary working as designed. We just report.
    match check_endpoint(&cli.forkd_endpoint) {
        Ok(()) => println!("  forkd endpoint: reachable (HTTP 2xx)"),
        Err(e) => println!("  forkd endpoint: UNREACHABLE — {e}"),
    }
}

fn check_endpoint(url: &str) -> Result<()> {
    let token = forkd_token()?;
    let client = fabro_http::blocking_http_client().context("build doctor http client")?;
    let resp = client
        .get(format!("{}/health", url.trim_end_matches('/')))
        .bearer_auth(token)
        .send()
        .with_context(|| format!("GET {url}/health"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("non-2xx ({})", resp.status())
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: tracing writes to std::io::stderr; no Tokio runtime here"
)]
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

// Tiny smoke test for the CLI's tier→route mapping. The full
// end-to-end matrix is covered in tests/.
#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn cli_parses_canary_subcommand() {
        let cli = Cli::parse_from([
            "fabro-referee",
            "canary",
            "--run-id",
            "r-abc",
            "--branch-base",
            "p0",
            "--green",
            "--backend",
            "hermetic",
        ]);
        match cli.command {
            Command::Canary {
                run_id,
                branch_base,
                green,
                ..
            } => {
                assert_eq!(run_id, "r-abc");
                assert_eq!(branch_base, "p0");
                assert!(green);
            }
            _ => panic!("expected Canary"),
        }
    }

    #[test]
    fn backend_kind_loose_match() {
        assert_eq!(
            BackendKind::from_str_loose("HERMETIC"),
            Some(BackendKind::Hermetic)
        );
        assert_eq!(
            BackendKind::from_str_loose("forkd"),
            Some(BackendKind::Forkd)
        );
        assert_eq!(BackendKind::from_str_loose("fake"), Some(BackendKind::Fake));
        assert_eq!(BackendKind::from_str_loose("nope"), None);
    }

    #[test]
    fn accept_refuses_unknown_backend() {
        let cli = Cli::parse_from(["fabro-referee", "--backend", "wat", "doctor"]);
        let kind = BackendKind::from_str_loose(&cli.backend);
        assert!(kind.is_none());
    }
}
