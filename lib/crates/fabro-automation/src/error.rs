use std::path::PathBuf;

use toml::de::Error as TomlDeError;

use crate::id::AutomationId;
use crate::model::AutomationRevision;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AutomationValidationError {
    #[error("invalid automation id: {0}")]
    InvalidAutomationId(String),
    #[error("invalid automation trigger id: {0}")]
    InvalidTriggerId(String),
    #[error("automation name must not be empty")]
    EmptyName,
    #[error("invalid repository slug: {0}")]
    InvalidRepositorySlug(String),
    #[error("invalid git ref selector: {0}")]
    InvalidGitRefSelector(String),
    #[error("invalid workflow selector: {0}")]
    InvalidWorkflowSelector(String),
    #[error("duplicate trigger id: {0}")]
    DuplicateTriggerId(String),
    #[error("at most one api trigger is allowed")]
    MultipleApiTriggers,
    #[error("invalid schedule expression: {0}")]
    InvalidScheduleExpression(String),
    #[error("invalid trigger shape: {0}")]
    InvalidTriggerShape(String),
    #[error("unknown trigger type: {0}")]
    UnknownTriggerType(String),
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationStoreError {
    #[error("automation not found: {0}")]
    NotFound(AutomationId),
    #[error("automation already exists: {0}")]
    AlreadyExists(AutomationId),
    #[error("automation revision mismatch")]
    RevisionMismatch {
        expected: AutomationRevision,
        actual:   AutomationRevision,
    },
    #[error(transparent)]
    Validation(#[from] AutomationValidationError),
    #[error("failed to parse automation TOML at {}: {source}", path.display())]
    Parse {
        path:   PathBuf,
        source: TomlDeError,
    },
    #[error("failed to serialize automation TOML: {0}")]
    Serialize(String),
    #[error("I/O error at {}: {source}", path.display())]
    Io {
        path:   PathBuf,
        source: std::io::Error,
    },
}

impl AutomationStoreError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn parse(path: impl Into<PathBuf>, source: TomlDeError) -> Self {
        Self::Parse {
            path: path.into(),
            source,
        }
    }
}
