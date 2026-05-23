Goal: # Legacy Sandbox Config Auto-Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically rewrite confidently migratable legacy `[run.sandbox]` config files to the named-environments syntax during startup.

**Architecture:** Keep legacy behavior isolated in a removable `fabro-config` module. The normal settings schema stays strict; only file-based loads get a temporary parse-failure recovery path that rewrites the file, writes a backup, warns, and then resumes normal parsing.

**Tech Stack:** Rust, `toml_edit`, existing `fabro-config` settings builders, `tracing`, `tempfile` tests, public docs under `docs/public`.

---

## File Structure

- Create `lib/crates/fabro-config/src/legacy_sandbox_migration.rs`
  - Owns detection, TOML rewriting, backup naming/writing, unsupported-key diagnostics, and tests for legacy mappings.
- Modify `lib/crates/fabro-config/src/lib.rs`
  - Register the module privately.
- Modify `lib/crates/fabro-config/Cargo.toml`
  - Add the existing workspace `toml_edit` dependency; current main does not depend on it from `fabro-config`.
- Modify `lib/crates/fabro-config/src/load.rs`
  - Add a small parse-failure hook that delegates to the migration module, then returns to normal parsing.
- Modify docs:
  - `docs/public/execution/environments.mdx`
  - Create `docs/public/changelog/2026-05-23.mdx` because current main's latest public changelog is `2026-05-22.mdx`.

## Migration Contract

Only migrate when all of these are true:

- The file is valid TOML as a document.
- `[run.sandbox]` exists.
- `[run.environment]` does not exist.
- `[environments.default]` does not exist.
- Every legacy sandbox key is in the supported mapping below.
- The migrated content parses successfully as `SettingsLayer`.

This is intentionally a file-load migration only. Current in-memory parsing behavior, including `legacy_run_sandbox_is_rejected` in `lib/crates/fabro-config/src/tests/resolve_run.rs`, should remain strict and unchanged.

Supported mappings:

| Legacy key | New key |
|---|---|
| `run.sandbox.provider` | `run.environment.id = "default"` and `environments.default.provider` |
| `run.sandbox.preserve` | `environments.default.lifecycle.preserve` |
| `run.sandbox.env` | `environments.default.env` |
| `run.sandbox.daytona.skip_clone = true` | `run.clone.enabled = false` |
| `run.sandbox.docker.skip_clone = true` | `run.clone.enabled = false` |
| `run.sandbox.daytona.auto_stop_interval = N` | `environments.default.lifecycle.auto_stop = "{N}m"` |
| `run.sandbox.daytona.labels` | `environments.default.labels` |
| `run.sandbox.daytona.snapshot.name` | `environments.default.image.ref` |
| `run.sandbox.daytona.snapshot.cpu` | `environments.default.resources.cpu` |
| `run.sandbox.daytona.snapshot.memory` | `environments.default.resources.memory` |
| `run.sandbox.daytona.snapshot.disk` | `environments.default.resources.disk` |
| `run.sandbox.daytona.snapshot.dockerfile` | `environments.default.image.dockerfile` |
| `run.sandbox.daytona.volumes[].volume_id` | `environments.default.volumes[].id` |
| `run.sandbox.daytona.volumes[].mount_path` | `environments.default.volumes[].mount_path` |
| `run.sandbox.daytona.volumes[].subpath` | `environments.default.volumes[].subpath` |
| `run.sandbox.docker.image` | `environments.default.image.ref` |
| `run.sandbox.docker.memory_limit` | `environments.default.resources.memory` |
| `run.sandbox.docker.cpu_quota` | `environments.default.resources.cpu` when divisible by `100000` |

Unsupported or ambiguous cases fail with a message shaped like:

```text
Legacy [run.sandbox] settings in <path> could not be auto-migrated.

Unsupported keys:
  - run.sandbox.daytona.foo
  - run.sandbox.docker.cpu_quota

Rename legacy sandbox configuration to [run.environment] and [environments.<slug>].
See docs/public/execution/environments.mdx.
```

Successful migration writes a backup next to the original file:

```text
settings.toml.legacy-sandbox-migration.bak
settings.toml.legacy-sandbox-migration.1.bak
settings.toml.legacy-sandbox-migration.2.bak
```

Successful migration emits:

```text
Migrated legacy [run.sandbox] settings in <path> to [run.environment] and [environments.default]. Backup written to <backup>. This temporary compatibility migration will be removed before v1.0.
```

## Task 1: Add the Migration Module Skeleton

**Files:**
- Create: `lib/crates/fabro-config/src/legacy_sandbox_migration.rs`
- Modify: `lib/crates/fabro-config/src/lib.rs`
- Modify: `lib/crates/fabro-config/Cargo.toml`

- [ ] **Step 1: Add the `toml_edit` dependency**

Add this to `[dependencies]` in `lib/crates/fabro-config/Cargo.toml`:

```toml
toml_edit.workspace = true
```

- [ ] **Step 2: Register the module privately**

Add this beside the other private modules in `lib/crates/fabro-config/src/lib.rs`:

```rust
mod legacy_sandbox_migration;
```

- [ ] **Step 3: Create the module API**

Create `lib/crates/fabro-config/src/legacy_sandbox_migration.rs` with this starting shape:

```rust
#![expect(
    clippy::disallowed_methods,
    reason = "temporary startup config migration uses synchronous file I/O before config is loaded"
)]

use std::fmt;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::{Error, Result, SettingsLayer};

pub(crate) const REMOVAL_NOTE: &str =
    "This temporary compatibility migration will be removed before v1.0.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacySandboxMigrationReport {
    pub(crate) contents:    String,
    pub(crate) backup_path: PathBuf,
    pub(crate) warning:     String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MigrationFailure {
    unsupported_keys: Vec<String>,
}

impl fmt::Display for MigrationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Legacy [run.sandbox] settings could not be auto-migrated.")?;
        writeln!(f)?;
        writeln!(f, "Unsupported keys:")?;
        for key in &self.unsupported_keys {
            writeln!(f, "  - {key}")?;
        }
        writeln!(f)?;
        write!(
            f,
            "Rename legacy sandbox configuration to [run.environment] and [environments.<slug>]. See docs/public/execution/environments.mdx."
        )
    }
}

pub(crate) fn migrate_settings_path(
    path: &Path,
    original_contents: &str,
) -> Result<Option<LegacySandboxMigrationReport>> {
    let Some(next_contents) = migrate_contents(original_contents, path)? else {
        return Ok(None);
    };

    next_contents
        .parse::<SettingsLayer>()
        .map_err(|err| Error::parse_file("Migrated settings file is invalid", path, err))?;

    let backup_path = next_backup_path(path);
    std::fs::write(&backup_path, original_contents).map_err(|source| {
        Error::other(format!(
            "writing legacy sandbox migration backup {}: {source}",
            backup_path.display()
        ))
    })?;
    std::fs::write(path, &next_contents).map_err(|source| {
        Error::other(format!(
            "writing migrated settings file {}: {source}",
            path.display()
        ))
    })?;

    let warning = format!(
        "Migrated legacy [run.sandbox] settings in {} to [run.environment] and [environments.default]. Backup written to {}. {REMOVAL_NOTE}",
        path.display(),
        backup_path.display()
    );

    Ok(Some(LegacySandboxMigrationReport {
        contents: next_contents,
        backup_path,
        warning,
    }))
}

fn migrate_contents(original_contents: &str, path: &Path) -> Result<Option<String>> {
    let mut doc = match original_contents.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return Ok(None),
    };

    if !has_legacy_run_sandbox(&doc) {
        return Ok(None);
    }
    if has_new_environment_config(&doc) {
        return Err(Error::other(format!(
            "Legacy [run.sandbox] settings in {} could not be auto-migrated because the file already contains [run.environment] or [environments.default]. Remove one config style and retry.",
            path.display()
        )));
    }

    migrate_document(&mut doc).map_err(|failure| {
        Error::other(format!(
            "Legacy [run.sandbox] settings in {} could not be auto-migrated.\n\n{}",
            path.display(),
            failure
        ))
    })?;

    Ok(Some(doc.to_string()))
}

fn has_legacy_run_sandbox(doc: &DocumentMut) -> bool {
    doc.get("run")
        .and_then(Item::as_table)
        .and_then(|run| run.get("sandbox"))
        .is_some()
}

fn has_new_environment_config(doc: &DocumentMut) -> bool {
    let has_run_environment = doc
        .get("run")
        .and_then(Item::as_table)
        .and_then(|run| run.get("environment"))
        .is_some();
    let has_default_environment = doc
        .get("environments")
        .and_then(Item::as_table)
        .and_then(|envs| envs.get("default"))
        .is_some();
    has_run_environment || has_default_environment
}

fn next_backup_path(path: &Path) -> PathBuf {
    let base = path.with_file_name(format!(
        "{}.legacy-sandbox-migration.bak",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings.toml")
    ));
    if !base.exists() {
        return base;
    }

    for index in 1.. {
        let candidate = path.with_file_name(format!(
            "{}.legacy-sandbox-migration.{index}.bak",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("settings.toml")
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("unbounded backup suffix search should return")
}
```

- [ ] **Step 4: Add placeholder-free private stubs that compile**

Add private helpers with `unimplemented!()` only inside tests disabled by `#[cfg(test)]` is not allowed. Instead, make `migrate_document` return the one known unsupported failure until Task 2 fills it:

```rust
fn migrate_document(_doc: &mut DocumentMut) -> std::result::Result<(), MigrationFailure> {
    Err(MigrationFailure {
        unsupported_keys: vec!["run.sandbox".to_string()],
    })
}
```

- [ ] **Step 5: Run the focused compile check**

Run:

```bash
cargo test -p fabro-config legacy_sandbox_migration --quiet
```

Expected: compiles; there may be zero tests in this module at this point.

## Task 2: Implement Provider-Only Migration

**Files:**
- Modify: `lib/crates/fabro-config/src/legacy_sandbox_migration.rs`

- [ ] **Step 1: Add tests for provider-only migration**

Add these tests inside `legacy_sandbox_migration.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use fabro_types::settings::run::EnvironmentProvider;

    fn migrate(source: &str) -> String {
        migrate_contents(source, Path::new("settings.toml"))
            .expect("migration should not error")
            .expect("legacy sandbox should migrate")
    }

    #[test]
    fn provider_only_daytona_config_migrates_to_default_environment() {
        let migrated = migrate(
            r#"
_version = 1

[run.sandbox]
provider = "daytona"
"#,
        );

        let settings = migrated
            .parse::<SettingsLayer>()
            .expect("migrated TOML should parse");
        let resolved = crate::WorkflowSettingsBuilder::from_layer(&settings)
            .expect("migrated settings should resolve")
            .run;

        assert_eq!(resolved.environment.id, "default");
        assert_eq!(resolved.environment.provider, EnvironmentProvider::Daytona);
        assert!(migrated.contains("[run.environment]"));
        assert!(migrated.contains("[environments.default]"));
        assert!(!migrated.contains("[run.sandbox]"));
    }

    #[test]
    fn non_legacy_config_is_not_migrated() {
        let migrated = migrate_contents("_version = 1\n", Path::new("settings.toml"))
            .expect("non-legacy TOML should not error");

        assert!(migrated.is_none());
    }
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cargo test -p fabro-config provider_only_daytona_config_migrates_to_default_environment --quiet
```

Expected: FAIL because `migrate_document` still returns unsupported `run.sandbox`.

- [ ] **Step 3: Replace `migrate_document` with provider migration**

Implement the initial migration:

```rust
fn migrate_document(doc: &mut DocumentMut) -> std::result::Result<(), MigrationFailure> {
    let Some(sandbox_item) = doc
        .get("run")
        .and_then(Item::as_table)
        .and_then(|run| run.get("sandbox"))
    else {
        return Ok(());
    };
    let Some(sandbox) = sandbox_item.as_table().cloned() else {
        return Err(MigrationFailure {
            unsupported_keys: vec!["run.sandbox".to_string()],
        });
    };

    let mut unsupported = Vec::new();
    for (key, _) in sandbox.iter() {
        if key != "provider" {
            unsupported.push(format!("run.sandbox.{key}"));
        }
    }
    if !unsupported.is_empty() {
        return Err(MigrationFailure {
            unsupported_keys: unsupported,
        });
    }

    let Some(provider) = sandbox.get("provider").and_then(Item::as_str) else {
        return Err(MigrationFailure {
            unsupported_keys: vec!["run.sandbox.provider".to_string()],
        });
    };

    set_value(path_table(doc, &["run", "environment"]), "id", Value::from("default"));
    set_value(
        path_table(doc, &["environments", "default"]),
        "provider",
        Value::from(provider),
    );

    remove_run_sandbox(doc);
    Ok(())
}

fn path_table<'a>(doc: &'a mut DocumentMut, path: &[&str]) -> &'a mut Table {
    let mut item = doc.as_item_mut();
    for segment in path {
        item = &mut item[segment];
        if !item.is_table() {
            *item = Item::Table(Table::new());
        }
    }
    item.as_table_mut().expect("path item should be a table")
}

fn set_value(table: &mut Table, key: &str, value: Value) {
    table[key] = Item::Value(value);
}

fn remove_run_sandbox(doc: &mut DocumentMut) {
    if let Some(run) = doc.get_mut("run").and_then(Item::as_table_mut) {
        run.remove("sandbox");
    }
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p fabro-config legacy_sandbox_migration --quiet
```

Expected: PASS.

## Task 3: Add Daytona and Docker Field Mappings

**Files:**
- Modify: `lib/crates/fabro-config/src/legacy_sandbox_migration.rs`

- [ ] **Step 1: Add tests for direct legacy field mappings**

Add tests that assert resolved behavior, not only string contents:

```rust
#[test]
fn daytona_snapshot_labels_lifecycle_and_volumes_migrate() {
    let migrated = migrate(
        r#"
_version = 1

[run.sandbox]
provider = "daytona"
preserve = true

[run.sandbox.env]
NODE_ENV = "development"

[run.sandbox.daytona]
auto_stop_interval = 30

[run.sandbox.daytona.labels]
repo = "fabro-sh/fabro"

[run.sandbox.daytona.snapshot]
name = "fabro-v11"
cpu = 8
memory = "16GB"
disk = "20GB"
dockerfile = { path = "Dockerfile" }

[[run.sandbox.daytona.volumes]]
volume_id = "vol_auth"
mount_path = "/home/daytona/.config"
subpath = "agents"
"#,
    );

    let settings = migrated.parse::<SettingsLayer>().expect("migrated TOML should parse");
    let resolved = crate::WorkflowSettingsBuilder::from_layer(&settings)
        .expect("migrated settings should resolve")
        .run
        .environment;

    assert_eq!(resolved.image.reference.as_deref(), Some("fabro-v11"));
    assert_eq!(resolved.resources.cpu, Some(8));
    assert_eq!(resolved.resources.memory.map(|size| size.as_bytes()), Some(16_000_000_000));
    assert_eq!(resolved.resources.disk.map(|size| size.as_bytes()), Some(20_000_000_000));
    assert!(resolved.lifecycle.preserve);
    assert_eq!(resolved.lifecycle.auto_stop.map(|duration| duration.as_std().as_secs()), Some(1800));
    assert_eq!(resolved.labels.get("repo").map(String::as_str), Some("fabro-sh/fabro"));
    assert_eq!(resolved.env.get("NODE_ENV").map(|value| value.as_source()).as_deref(), Some("development"));
    assert_eq!(resolved.volumes.len(), 1);
    assert_eq!(resolved.volumes[0].id, "vol_auth");
    assert_eq!(resolved.volumes[0].mount_path, "/home/daytona/.config");
    assert_eq!(resolved.volumes[0].subpath.as_deref(), Some("agents"));
}

#[test]
fn docker_image_memory_and_cpu_quota_migrate() {
    let migrated = migrate(
        r#"
_version = 1

[run.sandbox]
provider = "docker"

[run.sandbox.docker]
image = "buildpack-deps:noble"
memory_limit = "4GB"
cpu_quota = 200000
"#,
    );

    let settings = migrated.parse::<SettingsLayer>().expect("migrated TOML should parse");
    let resolved = crate::WorkflowSettingsBuilder::from_layer(&settings)
        .expect("migrated settings should resolve")
        .run
        .environment;

    assert_eq!(resolved.provider, EnvironmentProvider::Docker);
    assert_eq!(resolved.image.reference.as_deref(), Some("buildpack-deps:noble"));
    assert_eq!(resolved.resources.cpu, Some(2));
    assert_eq!(resolved.resources.memory.map(|size| size.as_bytes()), Some(4_000_000_000));
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cargo test -p fabro-config legacy_sandbox_migration --quiet
```

Expected: FAIL because the two new nested-mapping tests are not implemented yet.

- [ ] **Step 3: Implement table copying and value transforms**

Extend `migrate_document` so it:

- Allows top-level legacy keys `provider`, `preserve`, `env`, `daytona`, and `docker`.
- Copies `run.sandbox.env` into `environments.default.env`.
- Sets `environments.default.lifecycle.preserve` from `run.sandbox.preserve`.
- Handles provider-specific nested mappings only for the selected provider.
- Removes `run.sandbox` after successful migration.

Use helper functions with these signatures:

```rust
fn migrate_daytona(sandbox: &Table, env: &mut Table, unsupported: &mut Vec<String>);
fn migrate_docker(sandbox: &Table, env: &mut Table, unsupported: &mut Vec<String>);
fn copy_table(source: &Item, target: &mut Table);
fn copy_array_of_tables_with_volume_id(source: &Item, target: &mut Table, unsupported: &mut Vec<String>);
fn item_path_keys(prefix: &str, item: &Item, out: &mut Vec<String>);
```

Implementation rules:

- `auto_stop_interval` must be an integer. Store `format!("{minutes}m")`.
- `docker.cpu_quota` must be an integer divisible by `100000`; otherwise add `run.sandbox.docker.cpu_quota` to unsupported keys.
- For Daytona volumes, each array entry may contain only `volume_id`, `mount_path`, and `subpath`; rename `volume_id` to `id`.
- `daytona.snapshot.dockerfile` must be copied as the existing TOML value, preserving inline string or `{ path = "..." }`.
- When collecting unsupported nested keys, report full paths such as `run.sandbox.daytona.snapshot.foo`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p fabro-config legacy_sandbox_migration --quiet
```

Expected: PASS.

## Task 4: Add File Rewrite and Loader Hook

**Files:**
- Modify: `lib/crates/fabro-config/src/load.rs`
- Modify: `lib/crates/fabro-config/src/legacy_sandbox_migration.rs`

- [ ] **Step 1: Add file rewrite tests**

Add tests:

```rust
#[test]
fn migrate_settings_path_writes_backup_and_rewrites_original() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.toml");
    let original = r#"
_version = 1

[run.sandbox]
provider = "daytona"
"#;
    std::fs::write(&path, original).expect("write fixture");

    let report = migrate_settings_path(&path, original)
        .expect("migration should succeed")
        .expect("legacy config should migrate");

    let rewritten = std::fs::read_to_string(&path).expect("read rewritten settings");
    let backup = std::fs::read_to_string(&report.backup_path).expect("read backup");

    assert_eq!(backup, original);
    assert!(rewritten.contains("[run.environment]"));
    assert!(rewritten.contains("[environments.default]"));
    assert!(report.warning.contains("temporary compatibility migration"));
}

#[test]
fn existing_backup_uses_numbered_suffix() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.toml");
    std::fs::write(path.with_file_name("settings.toml.legacy-sandbox-migration.bak"), "old")
        .expect("write existing backup");

    let next = next_backup_path(&path);

    assert!(next.ends_with("settings.toml.legacy-sandbox-migration.1.bak"));
}
```

- [ ] **Step 2: Run tests and confirm current state**

Run:

```bash
cargo test -p fabro-config legacy_sandbox_migration --quiet
```

Expected: PASS if Task 1 file-writing code compiled; otherwise fix only the migration module.

- [ ] **Step 3: Hook migration into file loading**

Change `load_settings_path` in `lib/crates/fabro-config/src/load.rs` to this shape:

```rust
pub(crate) fn load_settings_path(path: &Path) -> Result<SettingsLayer> {
    let content = std::fs::read_to_string(path).map_err(|source| Error::read_file(path, source))?;
    let mut layer = match content.parse::<SettingsLayer>() {
        Ok(layer) => layer,
        Err(err) => match crate::legacy_sandbox_migration::migrate_settings_path(path, &content)? {
            Some(report) => {
                tracing::warn!("{}", report.warning);
                eprintln!("{}", report.warning);
                report.contents.parse::<SettingsLayer>().map_err(|err| {
                    Error::parse_file("Migrated settings file is invalid", path, err)
                })?
            }
            None => return Err(Error::parse_file("Failed to parse settings file", path, err)),
        },
    };
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    resolve_goal_file_paths(&mut layer, base_dir);
    Ok(layer)
}
```

- [ ] **Step 4: Run loader-level verification**

Add a test in `load.rs` under `#[cfg(test)]` if the file does not already have a test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use fabro_types::settings::run::EnvironmentProvider;

    #[test]
    fn load_settings_path_auto_migrates_legacy_sandbox_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            r#"
_version = 1

[run.sandbox]
provider = "daytona"
"#,
        )
        .expect("write legacy settings");

        let layer = load_settings_path(&path).expect("legacy settings should auto-migrate");
        let resolved = crate::WorkflowSettingsBuilder::from_layer(&layer)
            .expect("migrated settings should resolve")
            .run;

        assert_eq!(resolved.environment.provider, EnvironmentProvider::Daytona);
        assert!(std::fs::read_to_string(&path)
            .expect("read rewritten settings")
            .contains("[run.environment]"));
    }
}
```

Run:

```bash
cargo test -p fabro-config load_settings_path_auto_migrates_legacy_sandbox_file --quiet
```

Expected: PASS.

- [ ] **Step 5: Verify in-memory TOML parsing remains strict**

Run the existing current-main rejection test:

```bash
cargo test -p fabro-config legacy_run_sandbox_is_rejected --quiet
```

Expected: PASS. Do not weaken `SettingsLayer` deserialization to accept `run.sandbox`; only `load_settings_path` should rewrite files from disk.

## Task 5: Unsupported and Ambiguous Cases

**Files:**
- Modify: `lib/crates/fabro-config/src/legacy_sandbox_migration.rs`

- [ ] **Step 1: Add failure tests**

Add tests:

```rust
#[test]
fn existing_new_environment_config_is_ambiguous() {
    let err = migrate_contents(
        r#"
_version = 1

[run.environment]
id = "default"

[run.sandbox]
provider = "daytona"
"#,
        Path::new("settings.toml"),
    )
    .expect_err("mixed old and new config should fail");

    assert!(err.to_string().contains("already contains [run.environment]"));
}

#[test]
fn unsupported_keys_are_reported_with_full_paths() {
    let err = migrate_contents(
        r#"
_version = 1

[run.sandbox]
provider = "daytona"

[run.sandbox.daytona]
unknown = true
"#,
        Path::new("settings.toml"),
    )
    .expect_err("unsupported keys should fail migration");

    let rendered = err.to_string();
    assert!(rendered.contains("run.sandbox.daytona.unknown"));
    assert!(rendered.contains("docs/public/execution/environments.mdx"));
}

#[test]
fn unsupported_docker_cpu_quota_is_reported() {
    let err = migrate_contents(
        r#"
_version = 1

[run.sandbox]
provider = "docker"

[run.sandbox.docker]
cpu_quota = 250000
"#,
        Path::new("settings.toml"),
    )
    .expect_err("non-divisible cpu quota should fail migration");

    assert!(err.to_string().contains("run.sandbox.docker.cpu_quota"));
}
```

- [ ] **Step 2: Run failure tests**

Run:

```bash
cargo test -p fabro-config legacy_sandbox_migration --quiet
```

Expected: PASS.

## Task 6: Documentation

**Files:**
- Modify: `docs/public/execution/environments.mdx`
- Create: `docs/public/changelog/2026-05-23.mdx`

- [ ] **Step 1: Document temporary auto-migration**

Add this note near the top of `docs/public/execution/environments.mdx`, after the initial environment/sandbox distinction:

```mdx
<Warning>
Older pre-v1.0 config files that still use `[run.sandbox]` are temporarily auto-migrated when Fabro loads them from disk. Fabro writes a sibling `*.legacy-sandbox-migration.bak` file, rewrites the config to `[run.environment]` plus `[environments.default]`, and then continues startup.

This compatibility rewrite only handles direct field mappings. Unsupported legacy fields fail with a migration message that lists the keys to edit manually. The rewrite path will be removed before v1.0.
</Warning>
```

- [ ] **Step 2: Add a changelog note**

Create `docs/public/changelog/2026-05-23.mdx`:

```mdx
---
title: "Legacy sandbox config migration"
date: "2026-05-23"
---

## Legacy sandbox config auto-migration

Fabro now temporarily rewrites confidently migratable pre-v1.0 `[run.sandbox]` config files to the named environment syntax. A backup is written next to the original file before rewriting. Ambiguous or unsupported legacy keys fail with a targeted migration message instead of the generic TOML unknown-field error.
```

- [ ] **Step 3: Check docs references**

Run:

```bash
rg -n "\\[run\\.sandbox\\]|legacy-sandbox-migration|run\\.environment" docs/public/execution docs/public/changelog
```

Expected: remaining `[run.sandbox]` references are either historical changelog entries or explicit migration warnings.

## Task 7: Full Verification

**Files:**
- All files touched above.

- [ ] **Step 1: Run config crate tests**

Run:

```bash
cargo test -p fabro-config --quiet
```

Expected: PASS.

- [ ] **Step 2: Run formatting check**

Run:

```bash
cargo +nightly-2026-04-14 fmt --check --all
```

Expected: PASS.

- [ ] **Step 3: Optional workspace lint if formatting and tests pass**

Run:

```bash
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

Expected: PASS. If this is slow, record that it was not run and include the reason in the handoff.

## Acceptance Criteria

- Boot-time config loading rewrites simple legacy `[run.sandbox]` files without user action.
- The rewritten file uses `[run.environment] id = "default"` and `[environments.default]`.
- The original file is preserved in a sibling backup before rewrite.
- Unsupported legacy keys fail with a targeted migration message listing exact keys.
- Normal strict schema behavior remains unchanged for in-memory `from_toml` calls.
- All legacy migration code is isolated in `legacy_sandbox_migration.rs` and removable before v1.0.


## Completed stages
- **toolchain**: succeeded
  - Script: `command -v cargo >/dev/null || { curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && sudo ln -sf $HOME/.cargo/bin/* /usr/local/bin/; }; cargo --version 2>&1`
  - Output:
    ```
    cargo 1.95.0 (f2d3ce0bd 2026-03-21)
    ```
- **preflight_compile**: succeeded
  - Script: `cargo check -q --workspace 2>&1`
  - Output: (empty)
- **preflight_lint**: succeeded
  - Script: `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings 2>&1`
  - Output: (empty)
- **implement**: succeeded
  - Model: gpt-5.5, 148.2k tokens in / 31.9k out
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 86.2k tokens in / 29.2k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-config/src/legacy_sandbox_migration.rs, /home/daytona/workspace/fabro/lib/crates/fabro-config/src/load.rs


# Simplify: Code Review and Cleanup

Review changes vs. origin for reuse, quality, and efficiency. Fix any issues found.

## Phase 1: Identify Changes

Run git diff (or git diff HEAD if there are staged changes) to see what changed. If there are no git changes, review the most recently modified files that the user mentioned or that you edited earlier in this conversation.

## Phase 2: Launch Three Review Agents in Parallel

Use the Agent tool to launch all three agents concurrently in a single message. Pass each agent the full diff so it has the complete context.

### Agent 1: Code Reuse Review

For each change:

1. Search for existing utilities and helpers that could replace newly written code. Use Grep to find similar patterns elsewhere in the codebase — common locations are utility directories, shared modules, and files adjacent to the changed ones.
2. Flag any new function that duplicates existing functionality. Suggest the existing function to use instead.
3. Flag any inline logic that could use an existing utility — hand-rolled string manipulation, manual path handling, custom environment checks, ad-hoc type guards, and similar patterns are common candidates.

Note: This is a greenfield app, so focus on maximizing simplicity and don't worry about changing things to achieve it.

### Agent 2: Code Quality Review

Review the same changes for hacky patterns:

1. Redundant state: state that duplicates existing state, cached values that could be derived, observers/effects that could be direct calls
2. Parameter sprawl: adding new parameters to a function instead of generalizing or restructuring existing ones
3. Copy-paste with slight variation: near-duplicate code blocks that should be unified with a shared abstraction
4. Leaky abstractions: exposing internal details that should be encapsulated, or breaking existing abstraction boundaries
5. Stringly-typed code: using raw strings where constants, enums (string unions), or branded types already exist in the codebase

Note: This is a greenfield app, so be aggressive in optimizing quality.

### Agent 3: Efficiency Review

Review the same changes for efficiency:

1. Unnecessary work: redundant computations, repeated file reads, duplicate network/API calls, N+1 patterns
2. Missed concurrency: independent operations run sequentially when they could run in parallel
3. Hot-path bloat: new blocking work added to startup or per-request/per-render hot paths
4. Unnecessary existence checks: pre-checking file/resource existence before operating (TOCTOU anti-pattern) — operate directly and handle the error
5. Memory: unbounded data structures, missing cleanup, event listener leaks
6. Overly broad operations: reading entire files when only a portion is needed, loading all items when filtering for one

## Phase 3: Fix Issues

Wait for all three agents to complete. Aggregate their findings and fix each issue directly. If a finding is a false positive or not worth addressing, note it and move on — do not argue with the finding, just skip it.

When done, briefly summarize what was fixed (or confirm the code was already clean).