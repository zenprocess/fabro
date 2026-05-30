use std::fmt;

use serde::{Deserialize, Serialize};

use crate::SecretType;
use crate::settings::server::GithubIntegrationStrategy;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerBootstrapResponse {
    pub config_toml: String,
    pub secrets:     Vec<WorkerBootstrapSecret>,
    pub github:      WorkerBootstrapGithubIntegration,
}

impl fmt::Debug for WorkerBootstrapResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerBootstrapResponse")
            .field("config_toml_bytes", &self.config_toml.len())
            .field("secret_count", &self.secrets.len())
            .field("github", &self.github)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerBootstrapSecret {
    pub name:        String,
    pub value:       String,
    #[serde(rename = "type")]
    pub secret_type: SecretType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl fmt::Debug for WorkerBootstrapSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerBootstrapSecret")
            .field("name", &self.name)
            .field("value", &"[REDACTED]")
            .field("secret_type", &self.secret_type)
            .field("description", &self.description)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerBootstrapGithubIntegration {
    pub enabled:  bool,
    pub strategy: GithubIntegrationStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id:   Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug:     Option<String>,
}
