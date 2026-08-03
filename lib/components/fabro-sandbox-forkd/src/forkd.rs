//! Thin async client for the forkd controller HTTP API.
//!
//! Wire shape (forkd 0.5.2):
//! * `POST /v1/sandboxes`               `{snapshot_tag}` -> `[{id,
//!   snapshot_tag?}]`
//! * `GET  /v1/sandboxes/{id}`          -> 2xx / 404 (liveness)
//! * `DELETE /v1/sandboxes/{id}`        -> 2xx / 404 (idempotent: 404 ==
//!   already gone)
//! * `POST /v1/sandboxes/{id}/exec`     `{args, timeout_secs}` -> `{stdout,
//!   stderr, exit_code}`
//!
//! All requests are bearer-authenticated with the configured token.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::PluginError;

/// Trait so the test suite can swap in a mock HTTP responder without
/// touching the real network.
#[async_trait]
pub trait ForkdClient: Send + Sync {
    async fn create(
        &self,
        base_url: &str,
        token: &str,
        snapshot_tag: &str,
    ) -> Result<CreateSandboxResponse, PluginError>;

    async fn delete(&self, base_url: &str, token: &str, id: &str) -> Result<(), PluginError>;

    async fn exec(
        &self,
        base_url: &str,
        token: &str,
        id: &str,
        args: &[String],
        timeout_secs: u64,
    ) -> Result<ExecResponse, PluginError>;
}

/// `POST /v1/sandboxes` request body.
#[derive(Debug, Serialize)]
pub struct CreateSandboxRequest {
    pub snapshot_tag: String,
}

/// Defensive response shape for `POST /v1/sandboxes`.  forkd 0.5.2 returns
/// an array; the untagged enum lets us accept a bare object too.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CreateSandboxResponse {
    Array(Vec<SandboxEntry>),
    Single(SandboxEntry),
}

impl CreateSandboxResponse {
    pub fn into_first(self) -> Option<SandboxEntry> {
        match self {
            Self::Array(v) if v.is_empty() => None,
            Self::Array(mut v) => Some(v.remove(0)),
            Self::Single(entry) => Some(entry),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SandboxEntry {
    pub id:           String,
    #[serde(default)]
    pub snapshot_tag: Option<String>,
}

/// `POST /v1/sandboxes/{id}/exec` request body.
#[derive(Debug, Serialize)]
pub struct ExecRequest {
    pub args:         Vec<String>,
    pub timeout_secs: u64,
}

/// `POST /v1/sandboxes/{id}/exec` response body.
#[derive(Debug, Deserialize)]
pub struct ExecResponse {
    #[serde(default)]
    pub stdout:    Option<String>,
    #[serde(default)]
    pub stderr:    Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

/// Real reqwest-backed client.  Default for production.  Tests use
/// `mock()` instead.
#[derive(Debug, Default)]
pub struct HttpClient;

impl HttpClient {
    pub fn new() -> Self {
        Self
    }

    fn build() -> Result<reqwest::Client, PluginError> {
        // We construct a raw reqwest client here rather than going through
        // `fabro_http` because this crate is intentionally a standalone
        // subprocess with a single dependency: the forkd controller.  The
        // `disallowed_methods` lint is the global server-side policy, not
        // the plugin-spawn boundary.  See `docs/internal/server-secrets-strategy.md`.
        #[expect(
            clippy::disallowed_methods,
            reason = "Plugin subprocess owns its own HTTP client lifecycle; the global policy applies to the in-tree server code, not to plugin subprocesses."
        )]
        let client = reqwest::Client::builder()
            .timeout(Duration::from_mins(2))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| PluginError::Forkd(format!("build HTTP client: {e}")))?;
        Ok(client)
    }
}

#[async_trait]
impl ForkdClient for HttpClient {
    async fn create(
        &self,
        base_url: &str,
        token: &str,
        snapshot_tag: &str,
    ) -> Result<CreateSandboxResponse, PluginError> {
        let client = Self::build()?;
        let url = format!("{base_url}/v1/sandboxes");
        let body = CreateSandboxRequest {
            snapshot_tag: snapshot_tag.to_string(),
        };
        let resp = client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| PluginError::Forkd(format!("create HTTP send: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(PluginError::Forkd(format!(
                "create returned {status}: {text}"
            )));
        }
        resp.json::<CreateSandboxResponse>()
            .await
            .map_err(|e| PluginError::Forkd(format!("create parse: {e}")))
    }

    async fn delete(&self, base_url: &str, token: &str, id: &str) -> Result<(), PluginError> {
        let client = Self::build()?;
        let url = format!("{base_url}/v1/sandboxes/{id}");
        let resp = client
            .delete(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| PluginError::Forkd(format!("delete HTTP send: {e}")))?;
        let status = resp.status();
        // 404 == already gone (idempotent success).  See spec.
        if status == reqwest::StatusCode::NOT_FOUND || status.is_success() {
            Ok(())
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(PluginError::Forkd(format!(
                "delete returned {status}: {text}"
            )))
        }
    }

    async fn exec(
        &self,
        base_url: &str,
        token: &str,
        id: &str,
        args: &[String],
        timeout_secs: u64,
    ) -> Result<ExecResponse, PluginError> {
        let client = Self::build()?;
        let url = format!("{base_url}/v1/sandboxes/{id}/exec");
        let body = ExecRequest {
            args: args.to_vec(),
            timeout_secs,
        };
        let resp = client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| PluginError::Forkd(format!("exec HTTP send: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(PluginError::Forkd(format!(
                "exec returned {status}: {text}"
            )));
        }
        resp.json::<ExecResponse>()
            .await
            .map_err(|e| PluginError::Forkd(format!("exec parse: {e}")))
    }
}
