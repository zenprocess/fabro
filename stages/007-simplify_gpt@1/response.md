Completed the cleanup review and applied one focused fix.

What I fixed:
- Removed the newly added duplicate `TranscriptUsage` type.
- Reused the existing canonical `fabro_model::TokenCounts` for `TranscriptMessage.usage`.
- Removed the now-unused `TranscriptUsage` re-export from `fabro-types`.
- Fixed the missing final newline in `transcript.rs`.

Validation run:
- `cargo check -q -p fabro-types -p fabro-llm`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo test -q -p fabro-types transcript --lib`
- `cargo check -q --workspace`

All passed.