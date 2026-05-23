Review/cleanup completed.

Fixed:
- Added a ULID suffix to agent-generated internal question IDs so they include stage/tool-call context but are still unique/safe.
- Changed agent answer label mapping to use the persisted option list as the source of truth instead of trusting `selected_option` from the submission.
- Made `bounded_display_field` private to avoid exposing a helper as public API unnecessarily.

Validation run:
- `cargo test -q -p fabro-workflow interview_runtime -- --nocapture`
- `cargo test -q -p fabro-agent question_tools -- --nocapture`
- `cargo test -q -p fabro-agent question_tool_round -- --nocapture`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo check -q -p fabro-agent -p fabro-workflow`

All passed.