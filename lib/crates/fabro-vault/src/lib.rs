use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::{fmt, io};

use chrono::{DateTime, Utc};
use fabro_static::EnvVars;
pub use fabro_types::SecretType;
use fabro_types::{SecretMetadata, is_env_style_name};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SecretEntry {
    pub value:       String,
    #[serde(rename = "type", default)]
    pub secret_type: SecretType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at:  DateTime<Utc>,
    pub updated_at:  DateTime<Utc>,
}

#[derive(Debug)]
pub enum Error {
    InvalidName(String),
    NotFound(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(f, "invalid secret name: {name}"),
            Self::NotFound(name) => write!(f, "secret not found: {name}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Serde(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

#[derive(Debug)]
pub struct SecretStore {
    path:      PathBuf,
    mutations: Mutex<()>,
    entries:   RwLock<HashMap<String, SecretEntry>>,
}

impl SecretStore {
    pub async fn load(path: PathBuf) -> Result<Self, Error> {
        let entries = match tokio::fs::read_to_string(&path).await {
            Ok(contents) => serde_json::from_str(&contents)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(err) => return Err(io_context("read secrets", &path, &err).into()),
        };

        Ok(Self {
            path,
            mutations: Mutex::new(()),
            entries: RwLock::new(entries),
        })
    }

    pub async fn set(
        &self,
        name: &str,
        value: &str,
        secret_type: SecretType,
        description: Option<&str>,
    ) -> Result<SecretMetadata, Error> {
        Self::validate_name(name, secret_type)?;

        let _mutation = self.mutations.lock().await;
        let mut entries = self.entries.write().await;
        let now = Utc::now();
        let (created_at, description) = entries.get(name).map_or_else(
            || (now, description.map(str::to_string)),
            |entry| {
                (
                    entry.created_at,
                    description
                        .map(str::to_string)
                        .or_else(|| entry.description.clone()),
                )
            },
        );
        let entry = SecretEntry {
            value: value.to_string(),
            secret_type,
            description: description.clone(),
            created_at,
            updated_at: now,
        };
        let mut next_entries = entries.clone();
        next_entries.insert(name.to_string(), entry);
        self.write_atomic(&next_entries).await?;
        *entries = next_entries;

        Ok(SecretMetadata {
            name: name.to_string(),
            secret_type,
            description,
            created_at,
            updated_at: now,
        })
    }

    pub async fn remove(&self, name: &str) -> Result<(), Error> {
        let _mutation = self.mutations.lock().await;
        let mut entries = self.entries.write().await;
        if !entries.contains_key(name) {
            return Err(Error::NotFound(name.to_string()));
        }
        let mut next_entries = entries.clone();
        next_entries.remove(name);
        self.write_atomic(&next_entries).await?;
        *entries = next_entries;
        Ok(())
    }

    pub async fn list(&self) -> Vec<SecretMetadata> {
        let entries = self.entries.read().await;
        let mut data = entries
            .iter()
            .map(|(name, entry)| SecretMetadata {
                name:        name.clone(),
                secret_type: entry.secret_type,
                description: entry.description.clone(),
                created_at:  entry.created_at,
                updated_at:  entry.updated_at,
            })
            .collect::<Vec<_>>();
        data.sort_by(|a, b| a.name.cmp(&b.name));
        data
    }

    pub async fn get(&self, name: &str) -> Option<String> {
        self.entries
            .read()
            .await
            .get(name)
            .map(|entry| entry.value.clone())
    }

    pub fn try_get(&self, name: &str) -> Option<String> {
        self.entries
            .try_read()
            .ok()
            .and_then(|entries| entries.get(name).map(|entry| entry.value.clone()))
    }

    pub async fn get_entry(&self, name: &str) -> Option<SecretEntry> {
        self.entries.read().await.get(name).cloned()
    }

    pub async fn file_secrets(&self) -> Vec<(String, String)> {
        let entries = self.entries.read().await;
        let mut data = entries
            .iter()
            .filter(|(_, entry)| entry.secret_type == SecretType::File)
            .map(|(name, entry)| (name.clone(), entry.value.clone()))
            .collect::<Vec<_>>();
        data.sort_by(|a, b| a.0.cmp(&b.0));
        data
    }

    pub fn validate_name(name: &str, secret_type: SecretType) -> Result<(), Error> {
        match secret_type {
            SecretType::Token | SecretType::Oauth => Self::validate_env_name(name),
            SecretType::File => Self::validate_file_name(name),
        }
    }

    fn validate_env_name(name: &str) -> Result<(), Error> {
        if is_env_style_name(name) {
            Ok(())
        } else {
            Err(Error::InvalidName(name.to_string()))
        }
    }

    fn validate_file_name(name: &str) -> Result<(), Error> {
        if name == EnvVars::GITHUB_APP_PRIVATE_KEY {
            return Ok(());
        }

        if !name.starts_with('/') || name.ends_with('/') || name.contains('\0') {
            return Err(Error::InvalidName(name.to_string()));
        }

        let path = Path::new(name);
        if !path.is_absolute() {
            return Err(Error::InvalidName(name.to_string()));
        }

        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(Error::InvalidName(name.to_string()));
        }

        Ok(())
    }

    async fn write_atomic(&self, entries: &HashMap<String, SecretEntry>) -> Result<(), Error> {
        let parent = self
            .path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        tokio::fs::create_dir_all(&parent)
            .await
            .map_err(|err| io_context("create secrets directory", &parent, &err))?;

        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("secrets.json");
        let tmp_path = parent.join(format!(".{file_name}.tmp-{}", ulid::Ulid::new()));
        let json = serde_json::to_vec_pretty(entries)?;

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .await
            .map_err(|err| io_context("create secrets temp file", &tmp_path, &err))?;
        file.write_all(&json)
            .await
            .map_err(|err| io_context("write secrets temp file", &tmp_path, &err))?;
        file.sync_all()
            .await
            .map_err(|err| io_context("sync secrets temp file", &tmp_path, &err))?;
        drop(file);
        set_private_permissions(&tmp_path)?;
        tokio::fs::rename(&tmp_path, &self.path)
            .await
            .map_err(|err| {
                io_context(
                    &format!("rename secrets temp file to {}", self.path.display()),
                    &tmp_path,
                    &err,
                )
            })?;
        Ok(())
    }
}

/// Wrap an `io::Error` with a human-readable verb and path so downstream
/// reporting shows which operation failed on which file.
fn io_context(op: &str, path: &Path, source: &io::Error) -> io::Error {
    io::Error::new(source.kind(), format!("{op} {}: {source}", path.display()))
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| io_context("set permissions on", path, &err))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn load_missing_file_returns_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        assert!(store.list().await.is_empty());
    }

    #[tokio::test]
    async fn set_creates_entry_and_writes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let store = SecretStore::load(path.clone()).await.unwrap();

        let meta = store
            .set("OPENAI_API_KEY", "secret", SecretType::Token, None)
            .await
            .unwrap();

        assert_eq!(meta.name, "OPENAI_API_KEY");
        assert_eq!(meta.secret_type, SecretType::Token);
        assert_eq!(store.get("OPENAI_API_KEY").await.as_deref(), Some("secret"));
        assert!(path.exists());
    }

    #[tokio::test]
    async fn set_updates_existing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let store = SecretStore::load(path).await.unwrap();

        store
            .set("OPENAI_API_KEY", "first", SecretType::Token, None)
            .await
            .unwrap();
        store
            .set("OPENAI_API_KEY", "second", SecretType::Token, None)
            .await
            .unwrap();

        assert_eq!(
            store.get("OPENAI_API_KEY").await.as_deref(),
            Some("second")
        );
    }

    #[tokio::test]
    async fn remove_deletes_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let store = SecretStore::load(path.clone()).await.unwrap();
        store
            .set("OPENAI_API_KEY", "secret", SecretType::Token, None)
            .await
            .unwrap();

        store.remove("OPENAI_API_KEY").await.unwrap();

        assert_eq!(store.get("OPENAI_API_KEY").await, None);
    }

    #[tokio::test]
    async fn file_secrets_excludes_token_and_oauth_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        store
            .set("OPENAI_API_KEY", "token", SecretType::Token, None)
            .await
            .unwrap();
        store
            .set("OPENAI_CODEX", "oauth-json", SecretType::Oauth, None)
            .await
            .unwrap();
        store
            .set("/tmp/key.pem", "pem", SecretType::File, None)
            .await
            .unwrap();

        assert_eq!(store.file_secrets().await, vec![(
            "/tmp/key.pem".to_string(),
            "pem".to_string()
        )]);
    }

    #[tokio::test]
    async fn file_secret_listing_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let store = SecretStore::load(path.clone()).await.unwrap();
        store
            .set("/tmp/key.pem", "pem", SecretType::File, None)
            .await
            .unwrap();

        let reloaded = SecretStore::load(path).await.unwrap();
        assert_eq!(reloaded.file_secrets().await, vec![(
            "/tmp/key.pem".to_string(),
            "pem".to_string()
        )]);
    }

    #[tokio::test]
    async fn github_app_private_key_may_be_stored_as_file_secret() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();

        store
            .set(
                EnvVars::GITHUB_APP_PRIVATE_KEY,
                "base64-pem",
                SecretType::File,
                None,
            )
            .await
            .unwrap();

        assert_eq!(store.file_secrets().await, vec![(
            EnvVars::GITHUB_APP_PRIVATE_KEY.to_string(),
            "base64-pem".to_string()
        )]);
    }

    #[tokio::test]
    async fn list_includes_schema_typed_entries_loaded_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        tokio::fs::write(
            &path,
            serde_json::json!({
                "OPENAI_API_KEY": {
                    "value": "token",
                    "type": "token",
                    "created_at": "2026-04-12T00:00:00Z",
                    "updated_at": "2026-04-12T00:00:00Z"
                },
                "OPENAI_CODEX": {
                    "value": "{\"tokens\":{\"access_token\":\"access\",\"refresh_token\":\"refresh\",\"expires_at\":\"2026-04-12T01:00:00Z\"},\"config\":{\"auth_url\":\"https://auth.openai.com\",\"token_url\":\"https://auth.openai.com/oauth/token\",\"client_id\":\"client\",\"scopes\":[\"openid\"],\"redirect_uri\":null,\"use_pkce\":true}}",
                    "type": "oauth",
                    "created_at": "2026-04-12T00:00:00Z",
                    "updated_at": "2026-04-12T00:00:00Z"
                }
            })
            .to_string(),
        )
        .await
        .unwrap();

        let store = SecretStore::load(path).await.unwrap();

        let list = store.list().await;
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "OPENAI_API_KEY");
        assert_eq!(list[0].secret_type, SecretType::Token);
        assert_eq!(list[1].name, "OPENAI_CODEX");
        assert_eq!(list[1].secret_type, SecretType::Oauth);
        assert_eq!(store.get("OPENAI_API_KEY").await.as_deref(), Some("token"));
        assert!(store.get("OPENAI_CODEX").await.is_some());
    }

    #[tokio::test]
    async fn get_entry_returns_full_secret_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        store
            .set(
                "OPENAI_CODEX",
                "oauth-json",
                SecretType::Oauth,
                Some("saved auth"),
            )
            .await
            .unwrap();

        let entry = store.get_entry("OPENAI_CODEX").await.unwrap();

        assert_eq!(entry.value, "oauth-json");
        assert_eq!(entry.secret_type, SecretType::Oauth);
        assert_eq!(entry.description.as_deref(), Some("saved auth"));
    }

    #[tokio::test]
    async fn get_entry_returns_token_entries_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        store
            .set("OPENAI_API_KEY", "token", SecretType::Token, None)
            .await
            .unwrap();

        let entry = store.get_entry("OPENAI_API_KEY").await.unwrap();
        assert_eq!(entry.value, "token");
        assert_eq!(entry.secret_type, SecretType::Token);
    }
}
