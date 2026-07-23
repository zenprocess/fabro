//! Sink emitter — JSONL row + Markdown summary per accepted run.
//!
//! The sink is at `~/.ao/data/aofactory/referee/runs/<run-id>.jsonl`
//! (one JSONL row per route) and
//! `~/.ao/data/aofactory/referee/runs/<run-id>.md` (a human-readable
//! summary). The row shape is a forward-compatible superset of
//! zeninfra's `RefereeScore`; the harvest ETL consumes the JSONL.
//!
//! Flagged for zeninfra to confirm: whether `gate_backend` belongs in
//! `RefereeScore` or in a sibling `referee_meta` object. The current
//! shape lands it in the row so the canary can attribute the verdict
//! to the backend that produced it.

#[expect(
    clippy::disallowed_types,
    reason = "sync scorer binary: writeln! to a std::fs::File needs std::io::Write in scope; no Tokio runtime here"
)]
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::types::{RunRow, Verdict};

/// Default sink directory. Mirrored from the P0 spec.
#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: one-shot HOME lookup to locate the sink dir; no Tokio runtime here"
)]
pub fn default_sink_dir() -> PathBuf {
    let mut p = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    p.push(".ao");
    p.push("data");
    p.push("aofactory");
    p.push("referee");
    p.push("runs");
    p
}

/// Append one RunRow to `<run-id>.jsonl`. mkdir-p's the sink dir.
#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: append-open the JSONL sink; no Tokio runtime here"
)]
pub fn append_jsonl(sink_dir: &Path, row: &RunRow) -> Result<PathBuf> {
    std::fs::create_dir_all(sink_dir)
        .with_context(|| format!("create sink dir {}", sink_dir.display()))?;
    let path = sink_dir.join(format!("{}.jsonl", row.run_id));
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    let line = serde_json::to_string(row).context("serialize RunRow")?;
    writeln!(f, "{line}").with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// Write the Markdown summary for one run. Replaces the file if it
/// already exists (so re-running the same run_id is idempotent). The
/// body is operator-readable: per-route verdict, gate_log excerpt,
/// diff_stat, and the totals.
#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: create/replace the Markdown summary; no Tokio runtime here"
)]
pub fn write_markdown_summary(sink_dir: &Path, run_id: &str, rows: &[RunRow]) -> Result<PathBuf> {
    std::fs::create_dir_all(sink_dir)
        .with_context(|| format!("create sink dir {}", sink_dir.display()))?;
    let path = sink_dir.join(format!("{run_id}.md"));
    let mut f =
        std::fs::File::create(&path).with_context(|| format!("create {}", path.display()))?;
    let pass = rows.iter().filter(|r| r.verdict.is_pass()).count();
    let fail = rows.len() - pass;
    writeln!(f, "# Referee run `{run_id}`")?;
    writeln!(f)?;
    writeln!(
        f,
        "**Routes:** {} ({} pass, {} fail)",
        rows.len(),
        pass,
        fail
    )?;
    writeln!(f)?;
    writeln!(
        f,
        "| task_id | route | tier | tier_resolved | verdict | gate_backend | session_id |"
    )?;
    writeln!(f, "|---|---|---|---|---|---|---|")?;
    for r in rows {
        writeln!(
            f,
            "| {} | {} | {} | {} | {} | {} | {} |",
            r.task_id,
            r.route,
            r.tier,
            r.tier_resolved.as_deref().unwrap_or("?"),
            r.verdict,
            r.gate_backend,
            r.session_id.as_deref().unwrap_or("?"),
        )?;
    }
    writeln!(f)?;
    writeln!(f, "## Per-route gate logs")?;
    writeln!(f)?;
    for r in rows {
        writeln!(f, "### {} ({})", r.route, r.tier)?;
        writeln!(f)?;
        writeln!(f, "- **branch:** `{}`", r.branch)?;
        writeln!(f, "- **verdict:** `{}`", r.verdict)?;
        writeln!(f, "- **gate_backend:** `{}`", r.gate_backend)?;
        if let Some(vh) = &r.valset_hash {
            writeln!(f, "- **valset_hash:** `{vh}`")?;
        }
        if let Some(stat) = &r.diff_stat {
            writeln!(f, "- **diff_stat:** `{stat}`")?;
        }
        writeln!(f)?;
        writeln!(f, "<details><summary>gate_log</summary>")?;
        writeln!(f)?;
        writeln!(f, "```")?;
        writeln!(f, "{}", r.gate_log)?;
        writeln!(f, "```")?;
        writeln!(f)?;
        writeln!(f, "</details>")?;
        writeln!(f)?;
    }
    Ok(path)
}

impl Verdict {
    /// Convenience: `is_pass()` for the markdown summary.
    pub fn is_pass(self) -> bool {
        matches!(self, Self::Pass)
    }
}
