//! `fabro-sandbox-forkd` — JSON-RPC 2.0 over stdio sandbox-provider plugin for
//! the forkd microVM controller.
//!
//! This is a **reference implementation** of the upstream fabro-sh
//! provider-plugin sketch (PR #567).  The plugin is spawned by the host as a
//! subprocess and exchanges newline-delimited JSON-RPC 2.0 messages on
//! stdin/stdout.  **stdout is the protocol channel — do not write anything else
//! to it.**  All logging goes to stderr (and/or the `host/log` callback).
//!
//! The implementation is intentionally minimal: it covers the wire protocol
//! surface needed to demonstrate that the sketch works against a genuinely
//! different sandbox shape (a Firecracker microVM), and to surface three
//! places where the container-centric capability set does not line up with
//! forkd.  Those gaps are marked inline in this file (search for
//! `GAP 1` / `GAP 2` / `GAP 3`) — see the `gaps` module for the full
//! explanation.
//!
//! Out of scope (deliberately):
//! * The upstream `Sandbox` trait split / registry wiring / `PluginProvider`
//!   host side.
//! * Streaming exec (`exec/stream`, `exec/output` notifications) — forkd's exec
//!   is buffered; we declare `exec.streaming:false` and return the unsupported
//!   error.
//! * Native `fs/*` handlers — declared `fs.native:false`; the host derives them
//!   from exec (base64 cat/tee).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

pub mod capabilities;
pub mod forkd;
pub mod gaps;
pub mod protocol;

/// Errors that can occur inside the plugin.  Most errors are surfaced back to
/// the host as a JSON-RPC error response; the only case where the plugin
/// returns an outright failure to the host (instead of an error reply) is a
/// malformed read on stdin, which terminates the subprocess.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Forkd endpoint is not reachable, returned a non-2xx status, or its
    /// response could not be parsed.
    #[error("forkd controller error: {0}")]
    Forkd(String),

    /// The JSON-RPC request was structurally valid but the host asked for
    /// something this plugin does not support (e.g. `exec/stream`).
    #[error("this sandbox provider does not support it")]
    Unsupported,

    /// The request referenced a sandbox state that does not exist on the
    /// plugin side (e.g. exec before create).
    #[error("invalid state: {0}")]
    InvalidState(String),

    /// A JSON-RPC protocol violation (malformed envelope, missing id, etc.)
    #[error("json-rpc protocol error: {0}")]
    Protocol(String),

    /// Catch-all for I/O errors on the stdio channels.
    #[error("stdio error: {0}")]
    Stdio(String),
}

/// Per-sandbox state on the plugin side.  The plugin is single-sandbox-per-run
/// (it owns one microVM at a time) — this is a deliberate scoping choice for a
/// reference implementation; a production plugin would key a map of these by
/// `id`.
#[derive(Debug, Default)]
pub struct SandboxState {
    /// The server-assigned sandbox id from `POST /v1/sandboxes`.
    pub id:           Option<String>,
    /// The snapshot tag the server resolved for us (may differ from the
    /// requested tag — see forkd 0.5.2 contract).
    pub snapshot_tag: Option<String>,
}

/// The shared state of the plugin process.  Wrapped in an `Arc<Mutex<_>>` so
/// the JSON-RPC read loop and the per-request handler can both reach it.
#[derive(Debug, Default)]
pub struct PluginState {
    /// Forkd controller base URL (e.g. `http://127.0.0.1:8889`).
    pub forkd_url:            String,
    /// Bearer token sent to the forkd controller.  NEVER loaded from a real
    /// secret in this skeleton — tests construct the state directly.
    pub forkd_token:          String,
    /// Default snapshot tag used when `sandbox/create` does not specify one.
    pub default_snapshot_tag: String,
    /// The single sandbox this plugin process owns.
    pub sandbox:              Mutex<SandboxState>,
    /// Whether `initialize` has succeeded.  Everything else is rejected
    /// before this is set.
    pub initialized:          Mutex<bool>,
}

impl PluginState {
    /// Build a new plugin state from env.  The token is read from
    /// `FORKD_TOKEN`; the URL from `FORKD_URL`; the default snapshot tag from
    /// `FORKD_SNAPSHOT_TAG`.
    ///
    /// This is the only point in the crate that reads process env.  The
    /// plugin is a standalone subprocess whose entire configuration is
    /// delivered through env vars set by the host at spawn time; this is
    /// the documented `server-secrets-strategy` boundary for plugin
    /// subprocesses, not a process-wide env mutation.
    pub fn from_env() -> Self {
        // Read the three vars with `#[expect]` because reading a
        // process-env value at the plugin-spawn boundary IS the documented
        // env-var facade for plugin subprocesses.  See
        // `docs/internal/server-secrets-strategy.md`.
        #[expect(
            clippy::disallowed_methods,
            reason = "Plugin subprocess reads its configuration from env at spawn time; this is the documented env-var facade for plugin processes."
        )]
        let read = |name: &str, default: &str| -> String {
            std::env::var(name).unwrap_or_else(|_| default.to_string())
        };
        Self {
            forkd_url: read("FORKD_URL", "http://127.0.0.1:8889"),
            forkd_token: read("FORKD_TOKEN", ""),
            default_snapshot_tag: read("FORKD_SNAPSHOT_TAG", "default"),
            ..Self::default()
        }
    }
}

/// A JSON-RPC 2.0 request envelope.  `id` is `Option<Value>` because
/// notifications (no id) are legal.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method:  String,
    #[serde(default)]
    pub params:  Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id:      Option<Value>,
}

/// A JSON-RPC 2.0 success response.  `id` mirrors the request id.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub result:  Value,
    pub id:      Value,
}

impl JsonRpcResponse {
    pub fn ok(result: Value, id: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result,
            id,
        }
    }
}

/// A JSON-RPC 2.0 error response.  Per the spec, `code` is an integer and
/// `message` is a short string.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub jsonrpc: &'static str,
    pub error:   JsonRpcErrorBody,
    pub id:      Value,
}

impl JsonRpcError {
    pub fn for_request(request_id: Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            error:   JsonRpcErrorBody { code, message },
            id:      request_id,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcErrorBody {
    pub code:    i32,
    pub message: String,
}

/// Standard JSON-RPC 2.0 error codes we use.
pub mod error_code {
    /// JSON-RPC 2.0 standard code: invalid request.
    pub const INVALID_REQUEST: i32 = -32600;
    /// JSON-RPC 2.0 standard code: method not found.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// JSON-RPC 2.0 standard code: invalid params.
    pub const INVALID_PARAMS: i32 = -32602;
    /// JSON-RPC 2.0 standard code: internal error.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// Plugin-specific: the method is recognized but not supported by this
    /// provider.  The spec asks for the literal message
    /// "this sandbox provider does not support it".
    pub const UNSUPPORTED_BY_PROVIDER: i32 = -32001;
}

/// The heart of the plugin: a `JsonRpcRequest` dispatcher.
#[async_trait::async_trait]
pub trait RequestHandler: Send + Sync {
    /// Handle a single JSON-RPC request.  The return value is either a
    /// success result (serialized as the `result` field of the response) or
    /// a `PluginError` (serialized as the `error` field).
    async fn handle(
        &self,
        method: &str,
        params: Value,
        state: Arc<PluginState>,
    ) -> Result<Value, PluginError>;
}

/// The single dispatcher used by this plugin.  It owns the table of
/// method-name → handler and the plugin-wide state.
pub struct Plugin {
    pub state:   Arc<PluginState>,
    pub handler: Arc<dyn RequestHandler>,
}

impl Plugin {
    pub fn new(state: Arc<PluginState>, handler: Arc<dyn RequestHandler>) -> Self {
        Self { state, handler }
    }

    /// Dispatch a `JsonRpcRequest` and return a response (success or error).
    /// If the request is a notification (no `id`), this returns `None` and
    /// the response is not sent.
    pub async fn dispatch(&self, req: JsonRpcRequest) -> Option<serde_json::Value> {
        let id = req.id.clone();
        if req.jsonrpc != "2.0" {
            if let Some(id) = id {
                return Some(
                    serde_json::to_value(JsonRpcError::for_request(
                        id,
                        error_code::INVALID_REQUEST,
                        "jsonrpc must be \"2.0\"".to_string(),
                    ))
                    .expect("JsonRpcError serialization is infallible"),
                );
            }
            return None;
        }

        let result = self
            .handler
            .handle(&req.method, req.params, self.state.clone())
            .await;

        let id = id?;
        Some(match result {
            Ok(result) => serde_json::to_value(JsonRpcResponse::ok(result, id))
                .expect("JsonRpcResponse serialization is infallible"),
            Err(err) => {
                let (code, message) = match &err {
                    PluginError::Unsupported => (
                        error_code::UNSUPPORTED_BY_PROVIDER,
                        "this sandbox provider does not support it".to_string(),
                    ),
                    PluginError::InvalidState(msg) => (error_code::INVALID_PARAMS, msg.clone()),
                    PluginError::Protocol(msg) => (error_code::INVALID_REQUEST, msg.clone()),
                    PluginError::Forkd(msg) | PluginError::Stdio(msg) => {
                        (error_code::INTERNAL_ERROR, msg.clone())
                    }
                };
                if matches!(err, PluginError::Unsupported) {
                    debug!(method = req.method, "plugin: unsupported method");
                } else {
                    warn!(method = req.method, error = %err, "plugin: handler error");
                }
                serde_json::to_value(JsonRpcError::for_request(id, code, message))
                    .expect("JsonRpcError serialization is infallible")
            }
        })
    }

    /// Run the read loop on stdin / write loop on stdout.  Each line on stdin
    /// must be a complete JSON-RPC 2.0 envelope.  Responses (and notifications
    /// initiated by the plugin) are emitted as one JSON object per line on
    /// stdout.  Logs go to stderr.
    pub async fn run(self) -> Result<(), PluginError> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut reader = BufReader::new(stdin).lines();
        let mut writer = stdout;
        let mut out = String::new();

        info!(forkd_url = %self.state.forkd_url, "fabro-sandbox-forkd plugin started");

        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|e| PluginError::Stdio(e.to_string()))?
        {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let req: JsonRpcRequest = match serde_json::from_str(line) {
                Ok(req) => req,
                Err(err) => {
                    error!(error = %err, "plugin: malformed json-rpc request");
                    // Per JSON-RPC 2.0, an invalid request gets an error reply
                    // with id = null.  If the host never sent one, we still
                    // emit it so the host can see the parse failure.
                    let err = JsonRpcError::for_request(
                        Value::Null,
                        error_code::INVALID_REQUEST,
                        format!("malformed JSON-RPC request: {err}"),
                    );
                    let serialized = serde_json::to_string(&err)
                        .expect("JsonRpcError serialization is infallible");
                    out.clear();
                    out.push_str(&serialized);
                    out.push('\n');
                    writer
                        .write_all(out.as_bytes())
                        .await
                        .map_err(|e| PluginError::Stdio(e.to_string()))?;
                    writer
                        .flush()
                        .await
                        .map_err(|e| PluginError::Stdio(e.to_string()))?;
                    continue;
                }
            };

            // General-server-side ack: `shutdown` is a graceful exit.
            if req.method == "shutdown" {
                info!("plugin: shutdown requested");
                if let Some(id) = req.id {
                    let resp = JsonRpcResponse::ok(serde_json::json!({}), id);
                    let serialized = serde_json::to_string(&resp)
                        .expect("JsonRpcResponse serialization is infallible");
                    out.clear();
                    out.push_str(&serialized);
                    out.push('\n');
                    writer
                        .write_all(out.as_bytes())
                        .await
                        .map_err(|e| PluginError::Stdio(e.to_string()))?;
                    writer
                        .flush()
                        .await
                        .map_err(|e| PluginError::Stdio(e.to_string()))?;
                }
                return Ok(());
            }

            let response = self.dispatch(req).await;
            if let Some(response) = response {
                let serialized =
                    serde_json::to_string(&response).expect("response serialization is infallible");
                out.clear();
                out.push_str(&serialized);
                out.push('\n');
                writer
                    .write_all(out.as_bytes())
                    .await
                    .map_err(|e| PluginError::Stdio(e.to_string()))?;
                writer
                    .flush()
                    .await
                    .map_err(|e| PluginError::Stdio(e.to_string()))?;
            }
        }
        Ok(())
    }
}

/// Build the live handler with the default forkd HTTP client.
pub fn default_handler() -> Arc<dyn RequestHandler> {
    Arc::new(protocol::DefaultHandler::new(Arc::new(
        forkd::HttpClient::new(),
    )))
}

/// Common helper: pull a string field out of a JSON object, returning the
/// spec's invalid-params error if it's missing or not a string.
pub fn required_str<'a>(params: &'a Value, field: &str) -> Result<&'a str, PluginError> {
    params
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::InvalidState(format!("missing or non-string field: {field}")))
}

/// Common helper: pull an optional `u64` field out of a JSON object.
pub fn optional_u64(params: &Value, field: &str, default: u64) -> Option<u64> {
    params.get(field).and_then(Value::as_u64).or(Some(default))
}

/// A typed adapter for the `sandbox/delete` params: the id to delete is the
/// only required field.  Per spec, an unknown id is treated as success.
#[derive(Debug, Deserialize)]
pub struct DeleteParams {
    pub id: String,
}

/// A typed adapter for the `exec` params.  Per the spec, `exec` is
/// `{args:[string],timeout_secs:int?}`.
#[derive(Debug, Deserialize)]
pub struct ExecParams {
    pub args:         Vec<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    30
}

/// The exec result envelope plugin→host.  GAP 3 lives here — the upstream
/// sketch only models `termination: "exited"`, but forkd distinguishes
/// `ran` (the command completed — exit code is a real code verdict) from
/// `infra` (the sandbox could not be created/reached/exec'd/torn down) with
/// a `stage: boot | exec | teardown`.  Conflating them turns infrastructure
/// faults into code failures, which are sticky and poison downstream labels.
/// We surface the richer forkd shape inside the existing `termination` /
/// `exitCode` fields for now, and add an explicit `stage` +
/// `outcomeKind` field that the host can opt into.
#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub exit_code:    Option<i32>,
    pub stdout:       String,
    pub stderr:       String,
    /// Mirrors the upstream `termination: "exited"`.  Buffered-only plugin,
    /// so this is always "exited" today.
    pub termination:  &'static str,
    /// GAP 3 marker: which forkd stage produced this result.  `boot` is the
    /// sandbox-create round-trip, `exec` is the command execution, `teardown`
    /// is sandbox-delete.  Allows the host to SEPARATE infra failures from
    /// true code verdicts even when the wire-level exit code is non-zero.
    pub stage:        &'static str,
    /// GAP 3 marker: `ran` (the command legitimately ran — exit code is a
    /// real code verdict) or `infra` (the sandbox could not be
    /// created/reached/exec'd/torn down — does NOT count as a code verdict).
    pub outcome_kind: &'static str,
}

/// A typed adapter for the `sandbox/create` spec.  The upstream sketch
/// accepts a generic `SandboxSpec`; for this reference impl we model the
/// minimum forkd actually needs — `snapshot_tag`.
#[derive(Debug, Default, Deserialize)]
pub struct CreateParams {
    #[serde(default)]
    pub snapshot_tag: Option<String>,
}

/// The `sandbox/create` result envelope plugin→host.  Upstream shape:
/// `{id, state, runtime metadata}`.
#[derive(Debug, Serialize)]
pub struct CreateResult {
    pub id:           String,
    pub state:        String,
    pub snapshot_tag: String,
}

/// Helper used by the handler tests to seed a sandbox id.
#[doc(hidden)]
pub async fn set_sandbox_id(
    state: &PluginState,
    id: String,
    snapshot_tag: Option<String>,
) -> HashMap<&'static str, String> {
    let mut sb = state.sandbox.lock().await;
    sb.id = Some(id.clone());
    if let Some(ref tag) = snapshot_tag {
        sb.snapshot_tag = Some(tag.clone());
    }
    let mut out = HashMap::new();
    out.insert("id", id);
    if let Some(tag) = snapshot_tag {
        out.insert("snapshot_tag", tag);
    }
    out
}
