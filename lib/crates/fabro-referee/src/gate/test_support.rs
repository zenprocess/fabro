//! Test-only programmable fake gate backend. **Never** wired into the
//! production CLI's default backend selection; the only way to use
//! this is via `BackendKind::Fake` or by calling
//! `test_support::FakeBackend::default()` directly from a test.
//!
//! The fake backend satisfies the gate trait so the runner's
//! JSONL/sink plumbing can be exercised without `ao` or `dellsrv`.
//! It is also how the hermetic tests prove the **three-stage seam**
//! (run-routes → gate → emit) without depending on any external
//! system.

use std::sync::Mutex;

use anyhow::Result;

use super::{GateBackend, GateOutput};
use crate::types::{TaskSpec, Verdict};

/// Programmable in-process gate backend. Tests set
/// `verdict` / `gate_log` / `score` to whatever the test expects the
/// runner to receive; the runner then emits the JSONL row exactly as
/// it would for a real backend.
#[derive(Debug, Default)]
pub struct FakeBackend {
    inner: Mutex<FakeState>,
}

#[derive(Debug, Default, Clone)]
struct FakeState {
    verdict:     Option<Verdict>,
    gate_log:    Option<String>,
    score:       Option<f64>,
    valset_hash: Option<String>,
    /// Number of times `score()` was called. Tests use this to
    /// verify the runner actually invoked the gate once per route.
    calls:       usize,
}

impl FakeBackend {
    /// Lock + mutate the fake's programmed response. The test
    /// receives the number of calls observed so far.
    pub fn program(&self, verdict: Verdict, gate_log: impl Into<String>) {
        let mut s = self.inner.lock().unwrap();
        s.verdict = Some(verdict);
        s.gate_log = Some(gate_log.into());
        s.calls = 0;
    }

    pub fn calls(&self) -> usize {
        self.inner.lock().unwrap().calls
    }
}

impl GateBackend for FakeBackend {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn score(&self, _task: &TaskSpec, _route_diff: &str) -> Result<GateOutput> {
        let mut s = self.inner.lock().unwrap();
        s.calls += 1;
        let verdict = s
            .verdict
            .expect("FakeBackend: program() must be called before score()");
        let gate_log = s
            .gate_log
            .take()
            .unwrap_or_else(|| "FakeBackend: no log programmed".to_string());
        Ok(GateOutput {
            verdict,
            gate_log,
            backend: "fake".to_string(),
            score: s.score,
            valset_hash: s.valset_hash.take(),
        })
    }
}
