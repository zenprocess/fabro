Implemented cleanup after reviewing the migration changes.

Fixed:
- Backup writing now uses `OpenOptions::create_new(true)` to avoid a TOCTOU overwrite race when choosing numbered backup names.
- Backup path generation no longer panics on non-UTF-8 / missing file names; it falls back to `settings.toml`.
- Invalid legacy provider values are now reported as `run.sandbox.provider` instead of producing a later “migrated settings invalid” error.
- Unsupported key lists are sorted/deduplicated before rendering.
- Added a regression test for unsupported provider values.
- Addressed clippy findings from the cleanup.

Verification passed:
- `cargo test -p fabro-config legacy_sandbox_migration --quiet`
- `cargo test -p fabro-config load_settings_path_auto_migrates_legacy_sandbox_file --quiet`
- `cargo test -p fabro-config legacy_run_sandbox_is_rejected --quiet`
- `cargo test -p fabro-config --quiet`
- `cargo +nightly-2026-04-14 fmt --check --all`
- `cargo +nightly-2026-04-14 clippy -p fabro-config --all-targets -- -D warnings`
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`