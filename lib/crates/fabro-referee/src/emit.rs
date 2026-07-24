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
///
/// Idempotent: re-running the same `(run_id, route)` overwrites the
/// prior row (no double-append, no double-count). This is necessary
/// because the orchestrator may re-execute a canary against a
/// partially-failed route without bumping `run_id` — the
/// zeninfra episode-store sink must not double-count those replays.
///
/// Implementation: read every existing row for this `run_id`, drop
/// any row whose `(run_id, route)` matches the new one, append the
/// new row, and atomically rewrite the file. The markdown summary
/// (`write_markdown_summary`) already replaces the file on every
/// call, so it stays in sync without further changes.
#[expect(
    clippy::disallowed_methods,
    reason = "sync scorer binary: append-open the JSONL sink; no Tokio runtime here"
)]
pub fn append_jsonl(sink_dir: &Path, row: &RunRow) -> Result<PathBuf> {
    std::fs::create_dir_all(sink_dir)
        .with_context(|| format!("create sink dir {}", sink_dir.display()))?;
    let path = sink_dir.join(format!("{}.jsonl", row.run_id));
    // 1) read existing rows (ignore if the file is missing or empty)
    let mut existing: Vec<RunRow> = if path.exists() {
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read existing JSONL {}", path.display()))?;
        let mut rows = Vec::new();
        for (idx, line) in bytes.split(|b| *b == b'\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<RunRow>(line) {
                Ok(r) => rows.push(r),
                // Tolerate a corrupt trailing line (the previous
                // behaviour would also leave it on disk); log + skip.
                Err(e) => tracing::debug!(
                    line = idx,
                    err = %e,
                    "append_jsonl: dropping unparseable existing row"
                ),
            }
        }
        rows
    } else {
        Vec::new()
    };
    // 2) drop any prior row for the SAME (run_id, route) pair —
    // this is the idempotency contract.
    existing.retain(|r| !(r.run_id == row.run_id && r.route == row.route));
    // 3) append the new row
    existing.push(row.clone());
    // 4) write the whole file back atomically (write to a temp file
    // beside the target, then rename).  The rename is atomic on
    // POSIX, so a partial write never leaves a half-truncated JSONL.
    let mut serialized = String::new();
    for r in &existing {
        let line = serde_json::to_string(r).context("serialize RunRow")?;
        serialized.push_str(&line);
        serialized.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, serialized.as_bytes())
        .with_context(|| format!("write tmp {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
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
    let synthetic_count = rows.iter().filter(|r| r.synthetic).count();
    writeln!(f, "# Referee run `{run_id}`")?;
    writeln!(f)?;
    writeln!(
        f,
        "**Routes:** {} ({} pass, {} fail) | **synthetic:** {}/{}",
        rows.len(),
        pass,
        fail,
        synthetic_count,
        rows.len(),
    )?;
    writeln!(f)?;
    writeln!(
        f,
        "| task_id | route | tier | tier_resolved | verdict | gate_backend | session_id | synthetic |"
    )?;
    writeln!(f, "|---|---|---|---|---|---|---|---|")?;
    for r in rows {
        writeln!(
            f,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            r.task_id,
            r.route,
            r.tier,
            r.tier_resolved.as_deref().unwrap_or("?"),
            r.verdict,
            r.gate_backend,
            r.session_id.as_deref().unwrap_or("?"),
            if r.synthetic { "true" } else { "false" },
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

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "synchronous test fixture I/O; no Tokio runtime under cargo test"
)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::types::CURRENT_SCHEMA_VERSION;

    fn sink_tmp() -> PathBuf {
        std::env::temp_dir().join(format!("fabro-referee-emit-{}", ulid::Ulid::new()))
    }

    fn make_row(run_id: &str, route: &str, tier: &str, verdict: Verdict, gate_log: &str) -> RunRow {
        let mut r = RunRow::new(
            run_id, "T-emit", route, tier, "branch-x", verdict, "fake", gate_log,
        );
        r.ts = Utc::now();
        r
    }

    #[test]
    fn append_jsonl_writes_one_line_per_call() {
        let dir = sink_tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let _ = append_jsonl(
            &dir,
            &make_row("run-1", "mm", "minimax", Verdict::Pass, "ok-1"),
        )
        .unwrap();
        let _ = append_jsonl(
            &dir,
            &make_row("run-1", "sn", "sonnet", Verdict::Fail, "nope"),
        )
        .unwrap();
        let body = std::fs::read_to_string(dir.join("run-1.jsonl")).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(
            lines.len(),
            2,
            "two distinct (run_id, route) rows; body={body}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_jsonl_is_idempotent_per_run_id_route() {
        // Regression: re-running with the same (run_id, route) must
        // overwrite the prior row, never double-append.
        let dir = sink_tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let _ = append_jsonl(
            &dir,
            &make_row("run-replay", "mm", "minimax", Verdict::Pass, "first try"),
        )
        .unwrap();
        // Replay the same (run_id, route) with a different verdict —
        // simulates a re-run after a partial failure.
        let _ = append_jsonl(
            &dir,
            &make_row("run-replay", "mm", "minimax", Verdict::Fail, "second try"),
        )
        .unwrap();
        let body = std::fs::read_to_string(dir.join("run-replay.jsonl")).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "two calls for the same (run_id, route) MUST collapse to one row; body={body}"
        );
        // The surviving row must be the SECOND one (overwrite, not
        // first-wins).
        let parsed: RunRow = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed.gate_log, "second try");
        assert!(matches!(parsed.verdict, Verdict::Fail));
        // The schema_version field must be present and equal to the
        // current version (2 as of v2 — see `types.rs::CURRENT_SCHEMA_VERSION`).
        assert_eq!(parsed.schema_version, CURRENT_SCHEMA_VERSION);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_jsonl_does_not_cross_run_ids() {
        let dir = sink_tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let _ = append_jsonl(
            &dir,
            &make_row("run-a", "mm", "minimax", Verdict::Pass, "a"),
        )
        .unwrap();
        let _ = append_jsonl(
            &dir,
            &make_row("run-b", "mm", "minimax", Verdict::Pass, "b"),
        )
        .unwrap();
        let body_a = std::fs::read_to_string(dir.join("run-a.jsonl")).unwrap();
        let body_b = std::fs::read_to_string(dir.join("run-b.jsonl")).unwrap();
        assert_eq!(body_a.lines().count(), 1);
        assert_eq!(body_b.lines().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
