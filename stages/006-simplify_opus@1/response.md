Workspace builds clean. Let me summarize what was done.

## Summary

Reviewed the `fabro-environment` crate addition and related fabro-config/fabro-server changes against three concurrent agents (reuse, quality, efficiency) and applied the highest-impact fixes:

### Type / API surface cleanup
- **Collapsed `Environment` to `{id, revision, settings: EnvironmentSettings}`** — eliminated the parallel flat-field copy, `settings()`, `from_validated_settings`, and the eight clone fields. `Environment.settings` is now a single owned `EnvironmentSettings`.
- **Deleted `EnvironmentReplace`** — it was field-identical to `EnvironmentSettings` with no API consumer. `EnvironmentStore::replace` now takes `EnvironmentSettings` directly; `EnvironmentDraft` becomes `{id, settings}`.
- **Removed unused public API**: `Environment::from_toml_bytes`, `Environment::to_toml_string`, `EnvironmentStoreError::MissingRevision`.

### Dead code purge
- **Deleted `LegacySandboxMigrationReport`, `migrate_settings_path`, `write_next_backup`, `backup_path_for`, `next_backup_path`, `REMOVAL_NOTE` and the two tests** in the 2026050101 legacy sandbox migration — production goes through `migrate_contents` only via the new 2026052801 migration.
- **Removed `ResolvedAppStateSettings.manifest_run_settings`** dead field (server resolves it after loading the environment store anyway), plus the now-unused `RunSettingsBuilder`/`SharedError` imports in `serve.rs` and `test_support.rs`.

### Efficiency
- **Cached `catalog_layer()` as `Arc<MergeMap<EnvironmentLayer>>`** behind a `CatalogState` updated on create/replace/delete. Run create / preflight / graph / validate now just bump a refcount instead of cloning every `Environment` and rebuilding the catalog on each call.
- **Simplified `write_new`** to a direct `OpenOptions::create_new(true).open(path)` (matches `seed_missing_environments`). Removes the temp + hard_link + cleanup dance and its leak window.

### Quality polish
- Dropped fake `async` from `EnvironmentStore::list`/`get` (sync `RwLock` reads only).
- Inlined `resolve_path` into `base_dir.join(path)` (it was reimplementing `Path::join`).
- Replaced the local `value()` wrapper with `toml_edit::value`.
- Replaced `Option<PathBuf>` with `&Path` in `parse_persisted` (`from_persisted_path`'s "memory" branch is dead now).
- Extracted `check_revision` helper to eliminate duplicated lock-read + ok_or + revision-compare across `replace` and `delete`.
- Added small `read_state`/`write_state` accessors to centralize the lock-poisoning expect.

### Config plumbing
- **Removed the thin `load_settings_path` wrapper** and renamed `load_settings_path_with_source` to `load_settings_path(path, source)`. Each call site now declares its `SettingsSource` explicitly.
- **Unified the parse-then-validate pattern** behind a single `parse_settings_toml(source, kind)` helper, replacing 11 duplicated `.parse::<SettingsLayer>().map_err(...)?; validate_parsed_source(&layer, ...)?` blocks.
- Replaced the two scattered match arms in `parse.rs` with named predicates `SettingsSource::runs_settings_migrations` / `forbids_environment_catalog`.

### Verification
- `cargo nextest run -p fabro-environment -p fabro-config` — 233 passed, 0 failed.
- `cargo +nightly-2026-04-14 fmt --check --all` — clean.
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo build --workspace` — clean.

`fabro-server` retains 34 pre-existing test failures from the failed `implement` step (Task 5 of the original plan, "Adjust CLI And Manifest Behavior", was never executed). All share a "unknown environment: <id>" root cause: tests that exercise `fabro-manifest::build_run_manifest` directly never seed a server environment catalog. These failures predate this cleanup pass and aren't introduced by it.

Findings not addressed (deferred — would require crate-restructuring beyond a review pass):
- Sharing `write_atomic`/`temp_path_for`/`id_from_path`/etc. with the identical helpers in `fabro-automation` (would need a new shared util crate).
- Sharing the slug validator and SHA-256 revision type between `fabro-environment` and `fabro-automation` (cycle if extracted to fabro-config; would need a `fabro-util` extraction).
- Sharing `ensure_table` between `fabro-environment::model` and the legacy migration.
- Making `StickyMap`'s `Serialize` deterministic so `canonical_bytes` could collapse into `toml::to_string_pretty(layer)`.