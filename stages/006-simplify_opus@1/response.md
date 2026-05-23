Tree is clean, builds, and lints. Done.

## Summary

The `implement` stage failed mid-way and committed a small partial state (only added type definitions). I launched three parallel review agents (reuse / quality / efficiency) on the diff. Findings were consistent and serious:

| Axis | Top findings |
|---|---|
| **Reuse** | `EnvironmentProvider` duplicates `SandboxProvider`; `EnvironmentVolumeLayer` duplicates `DaytonaVolumeLayer`; `EnvironmentVolumeLayer` and `EnvironmentVolumeSettings` are identical; `EnvironmentNetworkSettings` regresses on `DaytonaNetworkLayer`'s tagged union; new `Combine for Option<Vec<…>>` impl bypasses the existing `impl_combine_or_option!` macro. |
| **Quality** | Code doesn't compile (`fabro_macros::Combine` derived on structs whose field types lack `Combine`/`SpliceMarker`); `RunEnvironmentSettings` dead-code; `RunNamespace` carries triple state (`environment_id`, `environment`, legacy `sandbox`) violating the plan's "no compat layer" rule; `EnvironmentLayer` and `RunEnvironmentLayer` are 95% copy-paste; unused imports; trailing newlines stripped from 8 files. |
| **Efficiency** | Three parallel sandbox representations on every `WorkflowSettings::default()`; `EnvironmentSettings` cloned twice (in catalog and inline); `to_environment_overlay()` clones unnecessarily; new `String::from("default")` heap-allocates on every default. |

Given the partial work was structurally flawed across all three axes and didn't compile, the simplest correct cleanup was to revert to the last known-good state (`preflight_compile` and `preflight_lint` passing). The revert commit message captures the findings so the next `implement` attempt can use them as design guidance.

**Result:** `cargo check --workspace` and `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` both pass clean.