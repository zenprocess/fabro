Looks good. Final summary:

## Summary

Three review agents analyzed the diff (canonical transcript types added in `fabro-types`, moved from `fabro-llm`, with new optional fields on agent event props).

### Fixes Applied

1. **Strum convention violation** — `MessageKind` and `MessageSource` in `lib/crates/fabro-types/src/transcript.rs` used plain `Serialize`/`Deserialize` with `#[serde(rename_all = "snake_case")]`. The codebase universally derives `strum::Display`, `strum::EnumString`, `strum::IntoStaticStr` for such enums (e.g. `PairStatus`, `PairSystemMessageKind`, every enum in `run_event/infra.rs`, etc.). Added matching strum derives + `#[strum(serialize_all = "snake_case")]`.

2. **Stringly-typed model identity** — `TranscriptMessage` had `provider: Option<String>` and `model: Option<String>` as two parallel string fields, even though `fabro_model::ModelRef` (already used by `AgentMessageProps.model`) bundles provider + model_id + speed in one strongly-typed struct. Replaced both fields with a single `model: Option<ModelRef>` so provider and model id can never drift apart. Updated the two tests that referenced the old fields.

3. **Missing trailing newline** in `lib/crates/fabro-llm/Cargo.toml` — added.

### Findings Intentionally Skipped

- **`ContentPart` hand-rolled `Deserialize` does a double-parse + deep clone** (efficiency reviewer): real but the code is pre-existing (just relocated from `fabro-llm`); cleaning it up requires splitting `Thinking`/`RedactedThinking` variants or writing a custom `MapAccess` visitor — wider scope than this review pass and not requested by the plan.
- **`default_tool_type()` String allocation**: pre-existing code, bounded frequency (tool-use turns, not per-token).
- **`BTreeMap` in `TranscriptUsage.extra`**: gives deterministic JSON key order — a feature for replayable events, not a bug.
- **Per-field doc comments**: codebase isn't uniformly doc-everything; existing docs on the public types are sufficient.

### Verification

- `cargo check --workspace --all-targets`: clean.
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo nextest run -p fabro-types`: 286/286 pass.
- `cargo nextest run -p fabro-llm -p fabro-workflow -p fabro-store -p fabro-server`: 2198/2200 pass; the 2 failures (`render_graph_from_manifest_*`, `get_graph_returns_svg`) are pre-existing environmental issues confirmed against `origin/main` (require a `dot`/graphviz render subprocess), not introduced by this diff.