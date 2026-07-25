//! Gate backend trait + the two production backends.
//!
//! The Referee scorer's gate is split into two concrete backends behind
//! one trait; the runner selects which backend to fire by config, and
//! the return shape is identical so the JSONL row is backend-agnostic.
//!
//! Production backends:
//!
//!   * [`backend::ForkdController`] — T3 real, scored via `dellsrv:8891`
//!     zen-gates `gate-run` / `forkd-exec`. Fully wired against the contract
//!     shape from the P0 spec but **NOT ACTIVATED** from this worktree (egress
//!     boundary; see `CONTRACTS.md` §1).
//!   * [`backend::HermeticLocal`] — contract-identical fallback. Applies the
//!     diff in a throwaway local git checkout, runs the task's closed-form
//!     acceptance, captures combined stdout/stderr as `gate_log`, exit 0 →
//!     pass, non-zero → fail. Never fakes a pass.
//!
//! Test-only backend:
//!
//!   * [`test_support::FakeBackend`] — a programmable in-process backend behind
//!     the same trait; lets the runner's JSONL/sink plumbing be exercised
//!     without `ao` or `dellsrv`. **Not in production.**
//!
//! The trait is synchronous: a gate is one short-lived operation per
//! route (one POST + one diff-apply + one acceptance run). Tokio would
//! add ceremony without a measurable throughput win at the canary scale
//! (2 routes × P0 canary).

use std::path::Path;

use anyhow::Result;

use crate::types::{GateOutput, TaskSpec};

pub mod backend;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// The gate's only contract. Both production backends and the
/// test-only fake implement this.
pub trait GateBackend: Send + Sync {
    /// Short backend name (e.g. `"forkd"`, `"hermetic"`, `"fake"`).
    /// Emitted in the JSONL row so the harvest ETL can attribute the
    /// score to the backend that produced it.
    fn name(&self) -> &'static str;

    /// Score one route. `route_diff` is the unified diff the runner
    /// captured from `git diff <base>...<branch>`. `task` carries the
    /// closed-form acceptance.
    ///
    /// MUST NOT return a `Pass` when the acceptance ran and exited
    /// non-zero (or when the diff failed to apply). The only legal
    /// `Pass` is one whose `gate_log` shows the acceptance exit 0.
    fn score(&self, task: &TaskSpec, route_diff: &str) -> Result<GateOutput>;
}

/// Configuration: which backend to use. CLI flag `--backend hermetic`
/// is the default for the canary (dellsrv is unreachable from this
/// worktree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// Real forkd controller at `dellsrv:8891`.
    Forkd,
    /// Contract-identical hermetic local gate.
    Hermetic,
    /// Test-only programmable backend. Refuses to run in production
    /// builds unless explicitly opt-in (`--backend fake`).
    Fake,
}

impl BackendKind {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "forkd" | "controller" | "zen-gates" => Some(Self::Forkd),
            "hermetic" | "local" | "fallback" => Some(Self::Hermetic),
            "fake" | "test" | "mock" => Some(Self::Fake),
            _ => None,
        }
    }

    /// Build the concrete backend. `forkd_endpoint` is unused for
    /// `Hermetic` / `Fake`; `valset_root` is the path the hermetic
    /// backend seeds the throwaway checkout from.
    ///
    /// The `Fake` arm is gated behind the `test-support` feature —
    /// it requires the `test_support` module to be compiled in. In a
    /// production build (no `test-support`), the `Fake` arm is
    /// dropped and any attempt to build it returns an error.
    pub fn build(self, forkd_endpoint: &str, valset_root: &Path) -> Result<Box<dyn GateBackend>> {
        match self {
            Self::Forkd => Ok(Box::new(backend::ForkdController::new(forkd_endpoint)?)),
            Self::Hermetic => Ok(Box::new(backend::HermeticLocal::new(
                valset_root.to_path_buf(),
            ))),
            #[cfg(feature = "test-support")]
            Self::Fake => Ok(Box::new(test_support::FakeBackend::default())),
            #[cfg(not(feature = "test-support"))]
            Self::Fake => {
                anyhow::bail!("Fake backend is test-only; rebuild with --features test-support")
            }
        }
    }
}
