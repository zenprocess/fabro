use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::RwLock;

use crate::error::AutomationStoreError;
use crate::id::AutomationId;
use crate::model::{
    Automation, AutomationDraft, AutomationPatch, AutomationReplace, AutomationRevision,
    PersistedAutomation,
};

#[derive(Debug)]
pub struct AutomationStore {
    dir:         PathBuf,
    automations: RwLock<BTreeMap<AutomationId, Automation>>,
}

impl AutomationStore {
    pub async fn load(dir: impl Into<PathBuf>) -> Result<Self, AutomationStoreError> {
        let dir = dir.into();
        tokio::task::spawn_blocking(move || Self::load_blocking(dir))
            .await
            .map_err(|err| {
                AutomationStoreError::io(
                    "<automation-store-loader>",
                    std::io::Error::other(err.to_string()),
                )
            })?
    }

    pub fn load_blocking(dir: impl Into<PathBuf>) -> Result<Self, AutomationStoreError> {
        let dir = dir.into();
        let mut automations = BTreeMap::new();

        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|err| AutomationStoreError::io(&dir, err))?;
                    let path = entry.path();
                    let metadata =
                        entry.metadata().map_err(|err| AutomationStoreError::io(&path, err))?;
                    if !metadata.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                        continue;
                    }

                    let id = automation_id_from_path(&path)?;
                    let bytes =
                        std::fs::read(&path).map_err(|err| AutomationStoreError::io(&path, err))?;
                    let persisted = parse_persisted(&path, &bytes)?;
                    let revision = revision_for_bytes(&bytes);
                    let automation = Automation::from_persisted(id.clone(), revision, persisted)?;
                    automations.insert(id, automation);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(AutomationStoreError::io(&dir, err)),
        }

        Ok(Self {
            dir,
            automations: RwLock::new(automations),
        })
    }

    pub async fn list(&self) -> Vec<Automation> {
        self.automations.read().await.values().cloned().collect()
    }

    pub async fn get(&self, id: &AutomationId) -> Option<Automation> {
        self.automations.read().await.get(id).cloned()
    }

    pub async fn create(
        &self,
        draft: AutomationDraft,
    ) -> Result<Automation, AutomationStoreError> {
        let mut automations = self.automations.write().await;
        if automations.contains_key(&draft.id) {
            return Err(AutomationStoreError::AlreadyExists(draft.id.to_string()));
        }

        let automation = draft.into_automation(AutomationRevision::new(""))?;
        let automation = self.persist(automation).await?;
        automations.insert(automation.id.clone(), automation.clone());
        Ok(automation)
    }

    pub async fn replace(
        &self,
        id: &AutomationId,
        expected: &AutomationRevision,
        draft: AutomationReplace,
    ) -> Result<Automation, AutomationStoreError> {
        ensure_revision_present(expected)?;
        let mut automations = self.automations.write().await;
        let existing = automations
            .get(id)
            .ok_or_else(|| AutomationStoreError::NotFound(id.to_string()))?;
        ensure_revision_matches(existing, expected)?;

        let automation = draft.into_automation(id.clone(), AutomationRevision::new(""))?;
        let automation = self.persist(automation).await?;
        automations.insert(id.clone(), automation.clone());
        Ok(automation)
    }

    pub async fn patch(
        &self,
        id: &AutomationId,
        expected: &AutomationRevision,
        patch: AutomationPatch,
    ) -> Result<Automation, AutomationStoreError> {
        ensure_revision_present(expected)?;
        let mut automations = self.automations.write().await;
        let existing = automations
            .get(id)
            .ok_or_else(|| AutomationStoreError::NotFound(id.to_string()))?;
        ensure_revision_matches(existing, expected)?;

        let automation = patch.apply_to(existing, AutomationRevision::new(""))?;
        let automation = self.persist(automation).await?;
        automations.insert(id.clone(), automation.clone());
        Ok(automation)
    }

    pub async fn delete(
        &self,
        id: &AutomationId,
        expected: &AutomationRevision,
    ) -> Result<(), AutomationStoreError> {
        ensure_revision_present(expected)?;
        let mut automations = self.automations.write().await;
        let existing = automations
            .get(id)
            .ok_or_else(|| AutomationStoreError::NotFound(id.to_string()))?;
        ensure_revision_matches(existing, expected)?;

        let path = self.path_for(id);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(AutomationStoreError::io(&path, err)),
        }
        automations.remove(id);
        Ok(())
    }

    fn path_for(&self, id: &AutomationId) -> PathBuf {
        self.dir.join(format!("{id}.toml"))
    }

    async fn persist(&self, mut automation: Automation) -> Result<Automation, AutomationStoreError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|err| AutomationStoreError::io(&self.dir, err))?;

        let bytes = canonical_toml_bytes(&automation)?;
        automation.revision = revision_for_bytes(&bytes);
        atomic_write(&self.dir, &self.path_for(&automation.id), &bytes).await?;
        Ok(automation)
    }
}

fn automation_id_from_path(path: &Path) -> Result<AutomationId, AutomationStoreError> {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Err(AutomationStoreError::InvalidFilename {
            path: path.to_path_buf(),
        });
    };
    AutomationId::try_from(stem.to_string()).map_err(AutomationStoreError::Validation)
}

fn parse_persisted(
    path: &Path,
    bytes: &[u8],
) -> Result<PersistedAutomation, AutomationStoreError> {
    let text = std::str::from_utf8(bytes).map_err(|err| {
        AutomationStoreError::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    toml::from_str(text).map_err(|source| AutomationStoreError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn canonical_toml_bytes(automation: &Automation) -> Result<Vec<u8>, AutomationStoreError> {
    let persisted = automation.to_persisted();
    let mut text = toml::to_string_pretty(&persisted)?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text.into_bytes())
}

fn revision_for_bytes(bytes: &[u8]) -> AutomationRevision {
    let digest = Sha256::digest(bytes);
    AutomationRevision::new(hex::encode(digest))
}

fn ensure_revision_present(expected: &AutomationRevision) -> Result<(), AutomationStoreError> {
    if expected.as_str().is_empty() {
        return Err(AutomationStoreError::MissingRevision);
    }
    Ok(())
}

fn ensure_revision_matches(
    automation: &Automation,
    expected: &AutomationRevision,
) -> Result<(), AutomationStoreError> {
    if &automation.revision != expected {
        return Err(AutomationStoreError::RevisionMismatch);
    }
    Ok(())
}

async fn atomic_write(dir: &Path, final_path: &Path, bytes: &[u8]) -> Result<(), AutomationStoreError> {
    let mut last_error = None;
    for attempt in 0..16_u8 {
        let temp_path = dir.join(temp_file_name(attempt));
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .await
        {
            Ok(mut file) => {
                file.write_all(bytes)
                    .await
                    .map_err(|err| AutomationStoreError::io(&temp_path, err))?;
                file.flush()
                    .await
                    .map_err(|err| AutomationStoreError::io(&temp_path, err))?;
                file.sync_all()
                    .await
                    .map_err(|err| AutomationStoreError::io(&temp_path, err))?;
                drop(file);
                if let Err(err) = tokio::fs::rename(&temp_path, final_path).await {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(AutomationStoreError::io(final_path, err));
                }
                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_error = Some(err);
            }
            Err(err) => return Err(AutomationStoreError::io(&temp_path, err)),
        }
    }

    Err(AutomationStoreError::io(
        dir,
        last_error.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "failed to allocate temporary automation file",
            )
        }),
    ))
}

fn temp_file_name(attempt: u8) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".automation-{nanos}-{attempt}.tmp")
}

#[cfg(test)]
mod tests {
    use crate::model::{ApiTrigger, GitRefSelector, RepositorySlug, WorkflowSlug};

    use super::*;

    fn id(value: &str) -> AutomationId {
        AutomationId::try_from(value).expect("valid automation id")
    }

    fn trigger_id(value: &str) -> crate::AutomationTriggerId {
        crate::AutomationTriggerId::try_from(value).expect("valid trigger id")
    }

    fn target(workflow: &str) -> crate::AutomationTarget {
        crate::AutomationTarget {
            repository: RepositorySlug::try_from("fabro-sh/fabro").expect("valid repo"),
            ref_:       GitRefSelector::try_from("main").expect("valid ref"),
            workflow:   WorkflowSlug::try_from(workflow).expect("valid workflow"),
        }
    }

    fn draft(id_value: &str) -> AutomationDraft {
        AutomationDraft {
            id:          id(id_value),
            name:        "Nightly dependency update".to_string(),
            description: Some("Open a PR for dependency updates.".to_string()),
            enabled:     None,
            target:      target("dependency-update"),
            triggers:    vec![crate::AutomationTrigger::Api(ApiTrigger {
                id:      trigger_id("api"),
                enabled: true,
            })],
        }
    }

    #[tokio::test]
    async fn missing_directory_loads_empty_store() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = AutomationStore::load(temp.path().join("automations"))
            .await
            .expect("store should load");

        assert!(store.list().await.is_empty());
    }

    #[tokio::test]
    async fn create_writes_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("automations");
        let store = AutomationStore::load(&dir).await.expect("store should load");

        let automation = store.create(draft("nightly-deps")).await.expect("create");

        let path = dir.join("nightly-deps.toml");
        assert!(path.is_file());
        let text = tokio::fs::read_to_string(path).await.expect("read file");
        assert!(text.contains("name = \"Nightly dependency update\""));
        assert_eq!(automation.revision.as_str().len(), 64);
    }

    #[tokio::test]
    async fn replace_changes_revision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = AutomationStore::load(temp.path().join("automations"))
            .await
            .expect("store should load");
        let automation = store.create(draft("nightly-deps")).await.expect("create");

        let replacement = AutomationReplace {
            name:        "Renamed".to_string(),
            description: automation.description.clone(),
            enabled:     automation.enabled,
            target:      automation.target.clone(),
            triggers:    automation.triggers.clone(),
        };
        let replaced = store
            .replace(&automation.id, &automation.revision, replacement)
            .await
            .expect("replace");

        assert_eq!(replaced.name, "Renamed");
        assert_ne!(replaced.revision, automation.revision);
    }

    #[tokio::test]
    async fn patch_keeps_unchanged_fields() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = AutomationStore::load(temp.path().join("automations"))
            .await
            .expect("store should load");
        let automation = store.create(draft("nightly-deps")).await.expect("create");

        let patch = AutomationPatch {
            description: Some(None),
            ..AutomationPatch::default()
        };
        let patched = store
            .patch(&automation.id, &automation.revision, patch)
            .await
            .expect("patch");

        assert_eq!(patched.name, automation.name);
        assert_eq!(patched.description, None);
        assert_eq!(patched.target, automation.target);
    }

    #[tokio::test]
    async fn stale_revision_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = AutomationStore::load(temp.path().join("automations"))
            .await
            .expect("store should load");
        let automation = store.create(draft("nightly-deps")).await.expect("create");
        let stale = AutomationRevision::new("stale");

        let err = store
            .replace(
                &automation.id,
                &stale,
                AutomationReplace {
                    name:        automation.name.clone(),
                    description: automation.description.clone(),
                    enabled:     automation.enabled,
                    target:      automation.target.clone(),
                    triggers:    automation.triggers.clone(),
                },
            )
            .await
            .expect_err("stale revision should fail");

        assert!(matches!(err, AutomationStoreError::RevisionMismatch));
    }

    #[tokio::test]
    async fn delete_removes_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("automations");
        let store = AutomationStore::load(&dir).await.expect("store should load");
        let automation = store.create(draft("nightly-deps")).await.expect("create");

        store
            .delete(&automation.id, &automation.revision)
            .await
            .expect("delete");

        assert!(!dir.join("nightly-deps.toml").exists());
        assert!(store.get(&automation.id).await.is_none());
    }

    #[tokio::test]
    async fn startup_fails_on_malformed_toml() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path().join("automations");
        tokio::fs::create_dir_all(&dir).await.expect("create dir");
        tokio::fs::write(dir.join("bad.toml"), "name =")
            .await
            .expect("write malformed file");

        let err = AutomationStore::load(&dir)
            .await
            .expect_err("malformed toml should fail");

        assert!(matches!(err, AutomationStoreError::Parse { .. }));
    }
}
