use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AutomationValidationError {
    #[error("invalid automation id: {0}")]
    InvalidAutomationId(String),
    #[error("invalid automation trigger id: {0}")]
    InvalidTriggerId(String),
    #[error("automation name cannot be empty")]
    EmptyName,
    #[error("invalid repository slug: {0}")]
    InvalidRepositorySlug(String),
    #[error("invalid git ref selector: {0}")]
    InvalidGitRef(String),
    #[error("invalid workflow selector: {0}")]
    InvalidWorkflowSelector(String),
    #[error("duplicate trigger id: {0}")]
    DuplicateTriggerId(String),
    #[error("at most one api trigger is allowed")]
    TooManyApiTriggers,
    #[error("invalid schedule expression: {0}")]
    InvalidScheduleExpression(String),
    #[error("unknown trigger type: {0}")]
    UnknownTriggerType(String),
}

#[derive(Debug, Error)]
pub enum AutomationStoreError {
    #[error("automation not found: {0}")]
    NotFound(String),
    #[error("automation already exists: {0}")]
    AlreadyExists(String),
    #[error("missing revision")]
    MissingRevision,
    #[error("revision mismatch")]
    RevisionMismatch,
    #[error(transparent)]
    Validation(#[from] AutomationValidationError),
    #[error("failed to parse automation file {path}: {source}")]
    Parse {
        path:   PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid automation filename: {path}")]
    InvalidFilename { path: PathBuf },
    #[error("I/O error at {path}: {source}")]
    Io {
        path:   PathBuf,
        source: std::io::Error,
    },
    #[error("failed to serialize automation: {0}")]
    Serialize(#[from] toml::ser::Error),
}

impl AutomationStoreError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
