use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{fmt, io};

use chrono::{DateTime, Utc};
use fabro_types::{Variable, is_env_style_name};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct VariableEntry {
    value:       String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    created_at:  DateTime<Utc>,
    updated_at:  DateTime<Utc>,
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
            Self::InvalidName(name) => write!(f, "invalid variable name: {name}"),
            Self::NotFound(name) => write!(f, "variable not found: {name}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Serde(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Serde(err) => Some(err),
            Self::InvalidName(_) | Self::NotFound(_) => None,
        }
    }
}

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
pub struct VariableStore {
    path:      PathBuf,
    mutations: Mutex<()>,
    entries:   RwLock<HashMap<String, VariableEntry>>,
}

impl VariableStore {
    pub async fn load(path: PathBuf) -> Result<Self, Error> {
        let entries = match tokio::fs::read_to_string(&path).await {
            Ok(contents) => serde_json::from_str(&contents)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(err) => return Err(io_context("read variables", &path, &err).into()),
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
        description: Option<&str>,
    ) -> Result<Variable, Error> {
        self.set_with_policy(name, value, description, false).await
    }

    pub async fn update_existing(
        &self,
        name: &str,
        value: &str,
        description: Option<&str>,
    ) -> Result<Variable, Error> {
        self.set_with_policy(name, value, description, true).await
    }

    async fn set_with_policy(
        &self,
        name: &str,
        value: &str,
        description: Option<&str>,
        require_existing: bool,
    ) -> Result<Variable, Error> {
        Self::validate_name(name)?;

        let _mutation = self.mutations.lock().await;
        let mut entries = self.entries.write().await;
        let now = Utc::now();
        let existing = entries.get(name);
        if require_existing && existing.is_none() {
            return Err(Error::NotFound(name.to_string()));
        }
        let (created_at, description) = existing.map_or_else(
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
        let entry = VariableEntry {
            value: value.to_string(),
            description: description.clone(),
            created_at,
            updated_at: now,
        };
        let mut next_entries = entries.clone();
        next_entries.insert(name.to_string(), entry);
        self.write_atomic(&next_entries).await?;
        *entries = next_entries;

        Ok(Variable {
            name: name.to_string(),
            value: value.to_string(),
            description,
            created_at,
            updated_at: now,
        })
    }

    pub async fn get(&self, name: &str) -> Option<Variable> {
        self.entries
            .read()
            .await
            .get(name)
            .map(|entry| variable_from_entry(name, entry))
    }

    pub async fn get_value(&self, name: &str) -> Option<String> {
        self.entries
            .read()
            .await
            .get(name)
            .map(|entry| entry.value.clone())
    }

    pub async fn list(&self) -> Vec<Variable> {
        let entries = self.entries.read().await;
        let mut data = entries
            .iter()
            .map(|(name, entry)| variable_from_entry(name, entry))
            .collect::<Vec<_>>();
        data.sort_by(|a, b| a.name.cmp(&b.name));
        data
    }

    pub async fn values_map(&self) -> HashMap<String, String> {
        self.entries
            .read()
            .await
            .iter()
            .map(|(name, entry)| (name.clone(), entry.value.clone()))
            .collect()
    }

    pub async fn remove(&self, name: &str) -> Result<(), Error> {
        Self::validate_name(name)?;
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

    pub fn validate_name(name: &str) -> Result<(), Error> {
        if is_env_style_name(name) {
            Ok(())
        } else {
            Err(Error::InvalidName(name.to_string()))
        }
    }

    async fn write_atomic(&self, entries: &HashMap<String, VariableEntry>) -> Result<(), Error> {
        let parent = self
            .path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        tokio::fs::create_dir_all(&parent)
            .await
            .map_err(|err| io_context("create variables directory", &parent, &err))?;

        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("variables.json");
        let tmp_path = parent.join(format!(".{file_name}.tmp-{}", ulid::Ulid::new()));
        let json = serde_json::to_vec_pretty(entries)?;
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .await
            .map_err(|err| io_context("create variables temp file", &tmp_path, &err))?;
        file.write_all(&json)
            .await
            .map_err(|err| io_context("write variables temp file", &tmp_path, &err))?;
        file.sync_all()
            .await
            .map_err(|err| io_context("sync variables temp file", &tmp_path, &err))?;
        drop(file);
        tokio::fs::rename(&tmp_path, &self.path)
            .await
            .map_err(|err| {
                io_context(
                    &format!("rename variables temp file to {}", self.path.display()),
                    &tmp_path,
                    &err,
                )
            })?;
        Ok(())
    }
}

fn variable_from_entry(name: &str, entry: &VariableEntry) -> Variable {
    Variable {
        name:        name.to_string(),
        value:       entry.value.clone(),
        description: entry.description.clone(),
        created_at:  entry.created_at,
        updated_at:  entry.updated_at,
    }
}

fn io_context(op: &str, path: &Path, source: &io::Error) -> io::Error {
    io::Error::new(source.kind(), format!("{op} {}: {source}", path.display()))
}
