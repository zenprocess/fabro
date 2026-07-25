//! # fabro-referee — the AO Factory Referee plane (P0)
//!
//! Per-attempt, per-tier scorer. One task → ≥2 tier routes → score
//! each route's diff through the forkd hermetic gate → emit
//! `{route/tier, pass|fail, gate_log}` (the GEPA textual-feedback
//! signal) to a readable sink.
//!
//! ## Architecture (seam-first)
//!
//! ```text
//!   runner::run()
//!     ├── emit::append_jsonl()       (one row per route)
//!     ├── emit::write_markdown_summary()
//!     └── gate::GateBackend::score()  (the only real seam)
//!             ├── backend::ForkdController (T3, real)
//!             └── backend::HermeticLocal  (contract-identical fallback)
//! ```
//!
//! The gate backend is selected by config (`BackendKind`). The
//! runner never fakes a pass — the verdict comes from the backend's
//! response. The hermetic tests use the `test_support::FakeBackend`
//! (feature-gated) to exercise the JSONL/sink plumbing without
//! needing `ao` or `dellsrv`.
//!
//! See `~/.ao/data/aofactory/referee/CONTRACTS.md` for the three
//! binding contracts; `CANARY-RUNBOOK.md` for the orchestrator's
//! live 2-tier canary.

pub mod canary;
pub mod decision_log;
pub mod emit;
pub mod gate;
pub mod runner;
pub mod types;

pub use gate::{BackendKind, GateBackend};
pub use runner::{RunResult, run, two_tier_canary_routes};
pub use types::{Acceptance, GateOutput, Route, RunRow, TaskSpec, Tier, Verdict};
