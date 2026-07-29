//! Optional external registration of referee rows in the Fabro run board.

use anyhow::{anyhow, Context, Result};
use serde_json::json;

use crate::types::{RunRow, Verdict};

const ENABLE_ENV: &str = "FABRO_REFEREE_REGISTER_RUNS";
const BASE_URL_ENV: &str = "FABRO_REFEREE_BASE_URL";
const HEAD_SHA_ENV: &str = "FABRO_REFEREE_HEAD_SHA";

/// Register a row when explicitly enabled. Failures are returned to the caller,
/// which deliberately logs them after the authoritative JSONL write succeeds.
pub fn register(row: &RunRow, base_ref: &str) -> Result<()> {
    if std::env::var(ENABLE_ENV).as_deref() != Ok("1") {
        return Ok(());
    }
    let base_url = std::env::var(BASE_URL_ENV)
        .with_context(|| format!("{BASE_URL_ENV} must be set when {ENABLE_ENV}=1"))?;
    let head_sha = std::env::var(HEAD_SHA_ENV)
        .with_context(|| format!("{HEAD_SHA_ENV} must be set when {ENABLE_ENV}=1"))?;
    validate_sha(base_ref, "base_ref")?;
    validate_sha(&head_sha, HEAD_SHA_ENV)?;

    let verdict = match row.verdict {
        Verdict::Pass => "pass",
        Verdict::Fail => "fail",
    };
    let dispatch_path = if row.backfill { "referee_backfill" } else { "auto_detect" };
    let kind = if row.backfill { "referee_backfill" } else { "referee" };
    let details = json!({
        "kind": kind,
        "tier": row.tier,
        "base_sha": base_ref,
        "head_sha": head_sha,
        "verdict": verdict,
        "verdict_at": row.ts,
        "dispatch_path": dispatch_path,
        "branch": row.branch,
        "synthetic": row.synthetic,
        "score": row.score,
        "valset_hash": row.valset_hash,
        "error": serde_json::Value::Null,
        "backfill_at": if row.backfill { json!(row.ts) } else { serde_json::Value::Null },
        "requester": if row.backfill { json!("fabro-referee") } else { serde_json::Value::Null },
    });
    let body = json!({ "run_id": row.run_id, "origin": { "kind": kind, "details": details } });
    let url = format!("{}/api/v1/runs/registrations", base_url.trim_end_matches('/'));
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .post(url)
        .json(&body)
        .send()
        .context("send referee run registration")?;
    if !response.status().is_success() {
        return Err(anyhow!("registration returned HTTP {}", response.status()));
    }
    Ok(())
}

fn validate_sha(value: &str, label: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(anyhow!("{label} must be a 40-character hexadecimal commit SHA, got {value:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ref_names() {
        assert!(validate_sha("HEAD", "base_ref").is_err());
        assert!(validate_sha(&"a".repeat(40), "base_ref").is_ok());
    }
}
