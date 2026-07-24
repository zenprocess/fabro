//! Schema-parity test: fabro's `RunRow` JSONL must carry every
//! field zeninfra's `GateLogLine` (Pydantic) contract requires, with
//! the right type, so a future rename or refactor breaks this test
//! loudly instead of silently dropping rows from the harvest ETL.
//!
//! Two fixtures in `tests/fixtures/`:
//!   * `runrow_v1_minimal.jsonl` — pre-alignment v1 row (no `attempt_key` /
//!     `model` / `passed`). Must still parse via `#[serde(default)]` for
//!     backwards-compat.
//!   * `runrow_v2_full.jsonl` — post-alignment v2 row with all three new fields
//!     populated. Must serialize with the correct top-level keys + JSON types.
//!
//! Required fields (zeninfra `GateLogLine`):
//!   `task_id`, `attempt_key`, `harness`, `model`, `passed`,
//!   `gate_log`, `ts`.
//! Optional fields: `score`, `valset_hash`.

use std::path::PathBuf;

use fabro_referee::decision_log::{DecisionLogLine, find_decision};
use fabro_referee::types::{CURRENT_SCHEMA_VERSION, RunRow};
use serde_json::Value;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(rel)
}

/// Read one fixture file as a String. Sync I/O — tests run under
/// `cargo test`, no Tokio runtime.
#[allow(
    clippy::disallowed_methods,
    reason = "sync test fixture I/O; no Tokio runtime under cargo test"
)]
fn read_fixture(rel: &str) -> String {
    std::fs::read_to_string(fixture(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Read one fixture file as a `Vec<serde_json::Value>` (one per
/// non-empty line).
#[allow(
    clippy::disallowed_methods,
    reason = "sync test fixture I/O; no Tokio runtime under cargo test"
)]
fn read_jsonl(rel: &str) -> Vec<Value> {
    read_fixture(rel)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("parse jsonl line"))
        .collect()
}

#[test]
fn v2_row_carries_every_zeninfra_required_field_with_correct_type() {
    let rows = read_jsonl("runrow_v2_full.jsonl");
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    let obj = r.as_object().expect("row must be a JSON object");

    // Top-level key set must include every GateLogLine REQUIRED
    // field. If a future rename breaks one of these, this assertion
    // fires before the harvest ETL silently drops the row.
    for key in [
        "task_id",
        "attempt_key",
        "harness",
        "model",
        "passed",
        "gate_log",
        "ts",
    ] {
        assert!(
            obj.contains_key(key),
            "missing required key `{key}`; top-level keys = {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }

    // Type parity (Rust ↔ Pydantic):
    //   task_id     str
    //   attempt_key str (deterministic per (task_id, run_id, route))
    //   harness     str
    //   model       str | null
    //   passed      bool
    //   gate_log    str
    //   ts          ISO-8601 string (serializes as RFC 3339)
    assert!(obj["task_id"].is_string());
    assert!(obj["attempt_key"].is_string());
    assert!(obj["harness"].is_string());
    assert!(
        obj["model"].is_string(),
        "model must be a string, not null, on a fully-resolved v2 row"
    );
    assert!(obj["passed"].is_boolean(), "passed must be a bool");
    assert!(obj["gate_log"].is_string());
    assert!(obj["ts"].is_string());

    // Schema version on the wire must match the constant; the
    // harvest ETL may pin on it.
    assert_eq!(
        obj["schema_version"].as_u64(),
        Some(u64::from(CURRENT_SCHEMA_VERSION))
    );
}

#[test]
fn v1_row_still_parses_via_serde_defaults() {
    // Backwards-compat: the 4 on-disk hermetic test fixtures
    // (p0-accept-{green,divergent}.jsonl under the operator's
    // ~/.ao/data/aofactory/referee/runs/) predate this PR and
    // carry NO attempt_key / model / passed. With serde defaults
    // they must still deserialize into RunRow.
    let rows = read_jsonl("runrow_v1_minimal.jsonl");
    assert_eq!(rows.len(), 1);
    let row: RunRow = serde_json::from_str(read_fixture("runrow_v1_minimal.jsonl").trim())
        .expect("v1 row must parse via serde defaults");

    // Defaults applied:
    assert_eq!(
        row.attempt_key, "",
        "v1 row: attempt_key defaults to empty string"
    );
    assert_eq!(row.model, None, "v1 row: model defaults to None");
    // Known limitation: passed defaults to `false` on v1 parse,
    // which is wrong for rows that were actually Verdict::Pass.
    // The on-disk operator fixtures are test artifacts and will be
    // re-emitted correctly on next canary.
    assert!(!row.passed);

    // Preserved v1 fields still parse correctly:
    assert_eq!(row.task_id, "T-canary-p0-stamp");
    assert_eq!(row.run_id, "p0-accept-green");
    assert_eq!(row.route, "mm");
    assert_eq!(row.branch, "p0-accept-mm");
    assert_eq!(row.harness, "claude-code");
    assert_eq!(row.gate_backend, "hermetic");
    assert!(!row.gate_log.is_empty());
}

#[test]
fn runrow_session_id_branch_join_with_wrapper_decisions_jsonl() {
    // Acceptance proof: the `(session_id, branch)` join keys
    // carried on RunRow actually line up with
    // `DecisionLogLine.session_id`+`branch` from
    // `wrapper-decisions.jsonl`. The harvester joins on these to
    // attach route-level treatment to attempt-level scoring.
    let dec_path = fixture("wrapper_decision_one_line.jsonl");
    let row: RunRow = serde_json::from_str(read_fixture("runrow_v2_full.jsonl").trim()).unwrap();
    let hit: DecisionLogLine = {
        let sid = row
            .session_id
            .as_deref()
            .expect("v2 fixture row carries session_id");
        find_decision(&dec_path, sid, &row.branch)
            .expect("decision-log join must find a matching line")
    };

    // Same (session_id, branch) on both sides:
    assert_eq!(hit.session_id, row.session_id.as_deref().unwrap());
    assert_eq!(hit.branch, row.branch);

    // And `final_model` (the wrapper side) lines up with what
    // `make_row` would have copied into `row.model`:
    assert_eq!(hit.final_model, "MiniMax-M3");
    // (We don't assert row.model == hit.final_model because the
    // fixture row was authored by hand for this test; the
    // end-to-end wiring is covered by the in-tree runner test
    // when a decision-log path is supplied. The point of THIS
    // test is to prove the join keys line up.)
}
