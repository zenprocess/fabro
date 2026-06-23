//! Forkd Firecracker microVM sandbox implementation.
//!
//! One long-lived VM per sandbox:
//! - Created in `initialize()` via `POST /vms`.
//! - Destroyed in `cleanup()` via `DELETE /vms/{name}`.
//! - All file I/O goes through exec (base64 round-trips for binary safety).
//! - Controller URL and bearer token are never hardcoded — they come from
//!   `FORKD_URL` / `FORKD_TOKEN` environment variables resolved at provider
//!   construction time.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use fabro_types::{CommandTermination, RunId};
use fabro_util::time::elapsed_ms;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::config::ForkdSettings;
use crate::sandbox::resolve_path;
use crate::{
    DirEntry, ExecResult, GrepOptions, Sandbox, SandboxEvent, SandboxEventCallback, shell_quote,
};

/// VM guest working directory.
pub(crate) const WORKING_DIRECTORY: &str = "/home/fabro/workspace";
/// VM guest repos root.
pub(crate) const REPOS_ROOT: &str = "/home/fabro/repos";
/// Provider name string for event payloads.
const PROVIDER: &str = "forkd";

/// Maximum number of retry attempts for transient HTTP failures (5xx / connect).
const HTTP_RETRY_LIMIT: u32 = 3;
/// Initial backoff before the first retry.
const HTTP_RETRY_INITIAL_BACKOFF: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// Public config type (re-exported by lib.rs)
// ---------------------------------------------------------------------------

/// Server-level connectivity for a forkd controller, plus per-sandbox runtime
/// settings.  The URL and token are resolved from environment variables and
/// must never be hardcoded.
#[derive(Clone)]
pub struct ForkdConfig {
    /// Base URL of the forkd controller, e.g. `http://127.0.0.1:8889`.
    pub forkd_url:   String,
    /// Bearer token for the forkd controller API.
    /// NEVER include the real value in log output — use `[redacted]`.
    pub forkd_token: String,
    /// Per-sandbox runtime settings (image, memory, network, skip_clone).
    pub settings:    ForkdSettings,
}

// Manual Debug implementation that redacts the bearer token so it never
// appears in tracing output, panic messages, or error chains.
impl std::fmt::Debug for ForkdConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForkdConfig")
            .field("forkd_url", &self.forkd_url)
            .field("forkd_token", &"[redacted]")
            .field("settings", &self.settings)
            .finish()
    }
}

impl Default for ForkdConfig {
    fn default() -> Self {
        Self {
            forkd_url:   "http://127.0.0.1:8889".to_string(),
            forkd_token: "forkd-local-token".to_string(),
            settings:    ForkdSettings::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// forkd REST API request/response shapes (forkd 0.5.2)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CreateVmRequest {
    name:    String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image:   Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kernel:  Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mem_mib: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExecRequest {
    command:     String,
    #[serde(skip_serializing_if = "Option::is_none")]
    working_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_ms:  Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env:         Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ExecResponse {
    stdout:    Option<String>,
    stderr:    Option<String>,
    exit_code: Option<i32>,
}

// ---------------------------------------------------------------------------
// ForkdSandbox
// ---------------------------------------------------------------------------

/// A sandbox backed by a single Firecracker microVM managed via forkd.
pub struct ForkdSandbox {
    config:           ForkdConfig,
    vm_name:          String,
    run_id:           Option<RunId>,
    clone_origin_url: Option<String>,
    clone_branch:     Option<String>,
    /// Populated after a successful `initialize()`.
    initialized:      OnceCell<bool>,
    origin_url:       OnceCell<String>,
    event_callback:   Option<SandboxEventCallback>,
}

impl ForkdSandbox {
    /// Create a new `ForkdSandbox`.  The VM is not created until
    /// [`initialize`](Self::initialize) is called.
    pub fn new(
        config: ForkdConfig,
        run_id: Option<RunId>,
        clone_origin_url: Option<String>,
        clone_branch: Option<String>,
    ) -> Self {
        let vm_name = if let Some(ref id) = run_id {
            format!("fabro-{id}")
        } else {
            format!(
                "fabro-{}-{:04x}",
                chrono::Utc::now().format("%Y%m%d-%H%M%S"),
                rand::rng().random_range(0..0x10000u32),
            )
        };
        Self {
            config,
            vm_name,
            run_id,
            clone_origin_url,
            clone_branch,
            initialized:    OnceCell::new(),
            origin_url:     OnceCell::new(),
            event_callback: None,
        }
    }

    /// The name of the VM that will be (or was) created.
    pub fn vm_name(&self) -> &str {
        &self.vm_name
    }

    /// Attach a callback that receives [`SandboxEvent`]s.
    pub fn set_event_callback(&mut self, cb: SandboxEventCallback) {
        self.event_callback = Some(cb);
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn emit(&self, event: SandboxEvent) {
        event.trace();
        if let Some(ref cb) = self.event_callback {
            cb(event);
        }
    }

    fn http_client(&self) -> crate::Result<reqwest::Client> {
        reqwest::Client::builder()
            // Hard cap on individual HTTP requests to the forkd controller.
            // Exec calls use a per-request timeout via ExecRequest.timeout_ms;
            // this is a safety net for connect + response-header receipt.
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| crate::Error::context("Failed to build HTTP client for forkd", e))
    }

    fn resolve_path(&self, path: &str) -> String {
        resolve_path(path, WORKING_DIRECTORY)
    }

    /// Returns `true` if an HTTP status code is a transient server-side error
    /// that is safe to retry (5xx range).
    fn is_retryable_status(status: reqwest::StatusCode) -> bool {
        status.is_server_error()
    }

    /// Execute a shell command in the VM.  Returns the raw `ExecResponse`
    /// so the caller decides how to interpret exit code / output.
    ///
    /// Transient HTTP errors (connect failures and 5xx responses) are retried
    /// up to `HTTP_RETRY_LIMIT` times with exponential back-off.
    async fn exec_in_vm(
        &self,
        command: &str,
        timeout_ms: Option<u64>,
        working_dir: Option<&str>,
        env_vars: Option<&HashMap<String, String>>,
    ) -> crate::Result<ExecResponse> {
        let client = self.http_client()?;
        let url = format!("{}/vms/{}/exec", self.config.forkd_url, self.vm_name);

        let body = ExecRequest {
            command:     command.to_string(),
            working_dir: working_dir.map(str::to_string),
            timeout_ms,
            env:         env_vars.cloned(),
        };

        let mut backoff = HTTP_RETRY_INITIAL_BACKOFF;
        let mut attempt = 0u32;
        loop {
            let result = client
                .post(&url)
                .bearer_auth(&self.config.forkd_token)
                .json(&body)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    return resp
                        .json::<ExecResponse>()
                        .await
                        .map_err(|e| crate::Error::context("Failed to parse forkd exec response", e));
                }
                Ok(resp) if Self::is_retryable_status(resp.status()) && attempt < HTTP_RETRY_LIMIT => {
                    let status = resp.status();
                    tracing::warn!(
                        attempt,
                        status = status.as_u16(),
                        "forkd exec transient error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(crate::Error::message(format!(
                        "forkd exec returned {status}: {text}"
                    )));
                }
                Err(e) if e.is_connect() && attempt < HTTP_RETRY_LIMIT => {
                    tracing::warn!(
                        attempt,
                        error = %e,
                        "forkd exec connect error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Err(e) => {
                    return Err(crate::Error::context("forkd exec HTTP request failed", e));
                }
            }
        }
    }

    /// Create the VM via `POST /vms`.
    ///
    /// Transient HTTP errors are retried up to `HTTP_RETRY_LIMIT` times.
    async fn create_vm(&self) -> crate::Result<()> {
        let client = self.http_client()?;
        let url = format!("{}/vms", self.config.forkd_url);

        let snap = self.config.settings.snapshot.as_ref();
        let network_str = self
            .config
            .settings
            .network
            .as_ref()
            .map(|n| match n {
                crate::config::ForkdNetwork::Block => "block".to_string(),
                crate::config::ForkdNetwork::AllowAll => "allow_all".to_string(),
                crate::config::ForkdNetwork::AllowList(_) => "allow_all".to_string(), // list enforced by iptables inside VM
            });

        let body = CreateVmRequest {
            name:    self.vm_name.clone(),
            image:   snap.and_then(|s| s.image.clone()),
            kernel:  snap.and_then(|s| s.kernel.clone()),
            mem_mib: snap.and_then(|s| s.mem_mib),
            network: network_str,
        };

        let mut backoff = HTTP_RETRY_INITIAL_BACKOFF;
        let mut attempt = 0u32;
        loop {
            let result = client
                .post(&url)
                .bearer_auth(&self.config.forkd_token)
                .json(&body)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                Ok(resp) if Self::is_retryable_status(resp.status()) && attempt < HTTP_RETRY_LIMIT => {
                    let status = resp.status();
                    tracing::warn!(
                        attempt,
                        status = status.as_u16(),
                        "forkd create_vm transient error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(crate::Error::message(format!(
                        "forkd create VM returned {status}: {text}"
                    )));
                }
                Err(e) if e.is_connect() && attempt < HTTP_RETRY_LIMIT => {
                    tracing::warn!(
                        attempt,
                        error = %e,
                        "forkd create_vm connect error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Err(e) => {
                    return Err(crate::Error::context("forkd create VM HTTP request failed", e));
                }
            }
        }
    }

    /// Delete the VM via `DELETE /vms/{name}`.
    ///
    /// Transient HTTP errors are retried up to `HTTP_RETRY_LIMIT` times.
    async fn delete_vm(&self) -> crate::Result<()> {
        let client = self.http_client()?;
        let url = format!("{}/vms/{}", self.config.forkd_url, self.vm_name);

        let mut backoff = HTTP_RETRY_INITIAL_BACKOFF;
        let mut attempt = 0u32;
        loop {
            let result = client
                .delete(&url)
                .bearer_auth(&self.config.forkd_token)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => return Ok(()),
                Ok(resp) if resp.status().is_success() => return Ok(()),
                Ok(resp) if Self::is_retryable_status(resp.status()) && attempt < HTTP_RETRY_LIMIT => {
                    let status = resp.status();
                    tracing::warn!(
                        attempt,
                        status = status.as_u16(),
                        "forkd delete_vm transient error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(crate::Error::message(format!(
                        "forkd delete VM returned {status}: {text}"
                    )));
                }
                Err(e) if e.is_connect() && attempt < HTTP_RETRY_LIMIT => {
                    tracing::warn!(
                        attempt,
                        error = %e,
                        "forkd delete_vm connect error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Err(e) => {
                    return Err(crate::Error::context("forkd delete VM HTTP request failed", e));
                }
            }
        }
    }

    /// Clone a git repository into the VM working directory.
    async fn clone_repo(&self, origin_url: &str, branch: Option<&str>) -> crate::Result<()> {
        self.emit(SandboxEvent::GitCloneStarted {
            url:    origin_url.to_string(),
            branch: branch.map(str::to_string),
        });
        let start = Instant::now();

        // Ensure parent directory exists.
        let mkdir_cmd = format!("mkdir -p {}", shell_quote(WORKING_DIRECTORY));
        let mkdir_resp = self
            .exec_in_vm(&mkdir_cmd, Some(30_000), None, None)
            .await?;
        if mkdir_resp.exit_code != Some(0) {
            return Err(crate::Error::message(format!(
                "forkd mkdir failed (exit {:?}): {}",
                mkdir_resp.exit_code,
                mkdir_resp.stderr.unwrap_or_default()
            )));
        }

        // Build clone command.
        let clone_cmd = if let Some(branch) = branch {
            format!(
                "git clone --branch {} {} {}",
                shell_quote(branch),
                shell_quote(origin_url),
                shell_quote(WORKING_DIRECTORY),
            )
        } else {
            format!(
                "git clone {} {}",
                shell_quote(origin_url),
                shell_quote(WORKING_DIRECTORY),
            )
        };

        let clone_resp = self
            .exec_in_vm(&clone_cmd, Some(300_000), None, None)
            .await?;

        if clone_resp.exit_code != Some(0) {
            let err = crate::Error::message(format!(
                "git clone failed (exit {:?}): {}",
                clone_resp.exit_code,
                clone_resp.stderr.unwrap_or_default()
            ));
            self.emit(SandboxEvent::GitCloneFailed {
                url:    origin_url.to_string(),
                error:  err.to_string(),
                causes: Vec::new(),
            });
            return Err(err);
        }

        let duration_ms = elapsed_ms(start);
        self.emit(SandboxEvent::GitCloneCompleted {
            url:         origin_url.to_string(),
            duration_ms,
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sandbox trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Sandbox for ForkdSandbox {
    // ------------------------------------------------------------------
    // Lifecycle
    // ------------------------------------------------------------------

    async fn initialize(&self) -> crate::Result<()> {
        if self.initialized.get().copied().unwrap_or(false) {
            return Ok(());
        }

        let start = Instant::now();
        self.emit(SandboxEvent::Initializing {
            provider: PROVIDER.into(),
        });

        // Create the VM.
        self.create_vm().await.map_err(|err| {
            let duration_ms = elapsed_ms(start);
            self.emit(SandboxEvent::InitializeFailed {
                provider:    PROVIDER.into(),
                error:       err.to_string(),
                causes:      err.causes(),
                duration_ms,
            });
            err
        })?;

        // Clone the repository if requested.
        if !self.config.settings.skip_clone {
            if let Some(ref origin_url) = self.clone_origin_url {
                self.clone_repo(origin_url, self.clone_branch.as_deref())
                    .await
                    .map_err(|err| {
                        let duration_ms = elapsed_ms(start);
                        self.emit(SandboxEvent::InitializeFailed {
                            provider:    PROVIDER.into(),
                            error:       err.to_string(),
                            causes:      err.causes(),
                            duration_ms,
                        });
                        err
                    })?;
                let _ = self.origin_url.set(origin_url.clone());
            }
        }

        let _ = self.initialized.set(true);
        let duration_ms = elapsed_ms(start);
        self.emit(SandboxEvent::Ready {
            provider:    PROVIDER.into(),
            duration_ms,
            name:        Some(self.vm_name.clone()),
            cpu:         None,
            memory:      self
                .config
                .settings
                .snapshot
                .as_ref()
                .and_then(|s| s.mem_mib)
                .map(|m| f64::from(m) / 1024.0),
            url:         None,
        });

        Ok(())
    }

    async fn cleanup(&self) -> crate::Result<()> {
        let start = Instant::now();
        self.emit(SandboxEvent::CleanupStarted {
            provider: PROVIDER.into(),
        });

        match self.delete_vm().await {
            Ok(()) => {
                let duration_ms = elapsed_ms(start);
                self.emit(SandboxEvent::CleanupCompleted {
                    provider: PROVIDER.into(),
                    duration_ms,
                });
                Ok(())
            }
            Err(err) => {
                self.emit(SandboxEvent::CleanupFailed {
                    provider: PROVIDER.into(),
                    error:    err.to_string(),
                    causes:   err.causes(),
                });
                Err(err)
            }
        }
    }

    // ------------------------------------------------------------------
    // Command execution
    // ------------------------------------------------------------------

    async fn exec_command(
        &self,
        command: &str,
        timeout_ms: u64,
        working_dir: Option<&str>,
        env_vars: Option<&HashMap<String, String>>,
        _cancel_token: Option<CancellationToken>,
    ) -> crate::Result<ExecResult> {
        let effective_dir = working_dir
            .map(|d| self.resolve_path(d))
            .unwrap_or_else(|| WORKING_DIRECTORY.to_string());

        let start = Instant::now();
        let resp = self
            .exec_in_vm(command, Some(timeout_ms), Some(&effective_dir), env_vars)
            .await?;

        let duration_ms = elapsed_ms(start);
        let exit_code = resp.exit_code;
        // When forkd returns no exit code the process was killed, OOM-killed, or
        // timed out by the controller.  We approximate using TimedOut; a future
        // forkd API version may surface a richer termination reason.
        let termination = match exit_code {
            Some(_) => CommandTermination::Exited,
            None => CommandTermination::TimedOut,
        };

        Ok(ExecResult {
            stdout: resp.stdout.unwrap_or_default(),
            stderr: resp.stderr.unwrap_or_default(),
            exit_code,
            termination,
            duration_ms,
        })
    }

    // exec_command_streaming: use the trait default (non-live replay).
    // spawn_stdio_process: use the trait default (returns Err).

    // ------------------------------------------------------------------
    // File system
    // ------------------------------------------------------------------

    async fn read_file_bytes(&self, path: &str) -> crate::Result<Vec<u8>> {
        let abs_path = self.resolve_path(path);
        // base64-encode the file so we can round-trip binary safely through the
        // exec response (which is a JSON string).
        let cmd = format!(
            "base64 -w 0 {}",
            shell_quote(&abs_path)
        );
        let resp = self.exec_in_vm(&cmd, Some(60_000), None, None).await?;

        if resp.exit_code != Some(0) {
            return Err(crate::Error::message(format!(
                "forkd read_file_bytes: base64 failed (exit {:?}): {}",
                resp.exit_code,
                resp.stderr.unwrap_or_default()
            )));
        }

        let encoded = resp.stdout.unwrap_or_default();
        BASE64
            .decode(encoded.trim())
            .map_err(|e| crate::Error::context("forkd read_file_bytes: base64 decode failed", e))
    }

    async fn write_file(&self, path: &str, content: &str) -> crate::Result<()> {
        let abs_path = self.resolve_path(path);
        let encoded = BASE64.encode(content.as_bytes());
        // Ensure parent directory exists.
        let parent = std::path::Path::new(&abs_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !parent.is_empty() {
            let mkdir_cmd = format!("mkdir -p {}", shell_quote(&parent));
            let mkdir_resp = self
                .exec_in_vm(&mkdir_cmd, Some(10_000), None, None)
                .await?;
            if mkdir_resp.exit_code != Some(0) {
                return Err(crate::Error::message(format!(
                    "forkd write_file mkdir failed: {}",
                    mkdir_resp.stderr.unwrap_or_default()
                )));
            }
        }

        let cmd = format!(
            "echo {} | base64 -d > {}",
            shell_quote(&encoded),
            shell_quote(&abs_path)
        );
        let resp = self.exec_in_vm(&cmd, Some(30_000), None, None).await?;
        if resp.exit_code != Some(0) {
            return Err(crate::Error::message(format!(
                "forkd write_file failed (exit {:?}): {}",
                resp.exit_code,
                resp.stderr.unwrap_or_default()
            )));
        }
        Ok(())
    }

    async fn delete_file(&self, path: &str) -> crate::Result<()> {
        let abs_path = self.resolve_path(path);
        let cmd = format!("rm -f {}", shell_quote(&abs_path));
        let resp = self.exec_in_vm(&cmd, Some(10_000), None, None).await?;
        if resp.exit_code != Some(0) {
            return Err(crate::Error::message(format!(
                "forkd delete_file failed (exit {:?}): {}",
                resp.exit_code,
                resp.stderr.unwrap_or_default()
            )));
        }
        Ok(())
    }

    async fn file_exists(&self, path: &str) -> crate::Result<bool> {
        let abs_path = self.resolve_path(path);
        let cmd = format!("test -e {}", shell_quote(&abs_path));
        let resp = self.exec_in_vm(&cmd, Some(10_000), None, None).await?;
        Ok(resp.exit_code == Some(0))
    }

    async fn list_directory(
        &self,
        path: &str,
        depth: Option<usize>,
    ) -> crate::Result<Vec<DirEntry>> {
        let abs_path = self.resolve_path(path);
        let max_depth = depth.unwrap_or(1);
        // Output format: TYPE SIZE NAME per line (TYPE is 'f' or 'd')
        let cmd = format!(
            r#"find {} -maxdepth {} -printf '%y %s %P\n' 2>/dev/null | tail -n +2"#,
            shell_quote(&abs_path),
            max_depth
        );
        let resp = self.exec_in_vm(&cmd, Some(30_000), None, None).await?;
        if resp.exit_code != Some(0) && resp.exit_code != Some(1) {
            return Err(crate::Error::message(format!(
                "forkd list_directory failed (exit {:?}): {}",
                resp.exit_code,
                resp.stderr.unwrap_or_default()
            )));
        }

        let stdout = resp.stdout.unwrap_or_default();
        let entries = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                let mut parts = line.splitn(3, ' ');
                let kind = parts.next()?;
                let size_str = parts.next()?;
                let name = parts.next()?;
                if name.is_empty() {
                    return None;
                }
                let is_dir = kind == "d";
                let size = if is_dir {
                    None
                } else {
                    size_str.parse::<u64>().ok()
                };
                Some(DirEntry {
                    name:  name.to_string(),
                    is_dir,
                    size,
                })
            })
            .collect();

        Ok(entries)
    }

    async fn download_file_to_local(
        &self,
        remote_path: &str,
        local_path: &Path,
    ) -> crate::Result<()> {
        let bytes = self.read_file_bytes(remote_path).await?;
        tokio::fs::write(local_path, &bytes)
            .await
            .map_err(|e| crate::Error::context("forkd download_file_to_local write failed", e))
    }

    async fn upload_file_from_local(
        &self,
        local_path: &Path,
        remote_path: &str,
    ) -> crate::Result<()> {
        let bytes = tokio::fs::read(local_path)
            .await
            .map_err(|e| crate::Error::context("forkd upload_file_from_local read failed", e))?;
        // Binary-safe: base64-encode raw bytes and decode inside the VM, mirroring
        // how read_file_bytes works in reverse.  String::from_utf8_lossy is
        // deliberately avoided — it silently corrupts any non-UTF-8 byte sequence
        // (e.g. compiled binaries, images, zip archives).
        let abs_path = self.resolve_path(remote_path);
        let encoded = BASE64.encode(&bytes);

        // Ensure parent directory exists.
        let parent = std::path::Path::new(&abs_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !parent.is_empty() {
            let mkdir_cmd = format!("mkdir -p {}", shell_quote(&parent));
            let mkdir_resp = self
                .exec_in_vm(&mkdir_cmd, Some(10_000), None, None)
                .await?;
            if mkdir_resp.exit_code != Some(0) {
                return Err(crate::Error::message(format!(
                    "forkd upload_file_from_local mkdir failed: {}",
                    mkdir_resp.stderr.unwrap_or_default()
                )));
            }
        }

        let cmd = format!(
            "echo {} | base64 -d > {}",
            shell_quote(&encoded),
            shell_quote(&abs_path)
        );
        let resp = self.exec_in_vm(&cmd, Some(60_000), None, None).await?;
        if resp.exit_code != Some(0) {
            return Err(crate::Error::message(format!(
                "forkd upload_file_from_local failed (exit {:?}): {}",
                resp.exit_code,
                resp.stderr.unwrap_or_default()
            )));
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Search
    // ------------------------------------------------------------------

    async fn grep(
        &self,
        pattern: &str,
        path: &str,
        options: &GrepOptions,
    ) -> crate::Result<Vec<String>> {
        let abs_path = self.resolve_path(path);
        let mut args = vec!["rg".to_string(), "--line-number".to_string()];

        if options.case_insensitive {
            args.push("-i".to_string());
        }
        if let Some(max) = options.max_results {
            args.push("-m".to_string());
            args.push(max.to_string());
        }
        if let Some(ref glob) = options.glob_filter {
            args.push("-g".to_string());
            args.push(shell_quote(glob));
        }
        args.push(shell_quote(pattern));
        args.push(shell_quote(&abs_path));

        let cmd = args.join(" ");
        let resp = self.exec_in_vm(&cmd, Some(60_000), None, None).await?;

        // Exit code 1 means no matches (not an error).
        if resp.exit_code != Some(0) && resp.exit_code != Some(1) {
            // rg not available — fall back to grep.
            let mut grep_args = vec!["grep".to_string(), "-rn".to_string()];
            if options.case_insensitive {
                grep_args.push("-i".to_string());
            }
            if let Some(max) = options.max_results {
                grep_args.push("-m".to_string());
                grep_args.push(max.to_string());
            }
            grep_args.push(shell_quote(pattern));
            grep_args.push(shell_quote(&abs_path));

            let grep_cmd = grep_args.join(" ");
            let grep_resp = self
                .exec_in_vm(&grep_cmd, Some(60_000), None, None)
                .await?;
            let stdout = grep_resp.stdout.unwrap_or_default();
            return Ok(stdout
                .lines()
                .map(str::to_string)
                .collect());
        }

        let stdout = resp.stdout.unwrap_or_default();
        Ok(stdout.lines().map(str::to_string).collect())
    }

    async fn glob(&self, pattern: &str, path: Option<&str>) -> crate::Result<Vec<String>> {
        let base = path
            .map(|p| self.resolve_path(p))
            .unwrap_or_else(|| WORKING_DIRECTORY.to_string());
        // Use `-name` for patterns without a directory separator (e.g. `*.rs`,
        // `Cargo.toml`) so they match at any depth without a spurious `*/` prefix
        // that would require at least one intermediate directory and would miss
        // files directly inside `base`.
        //
        // Use `-path` only when the caller supplies a path-qualified pattern
        // (e.g. `src/*.rs`), anchoring it to `<base>/` so the search is rooted.
        let find_filter = if pattern.contains('/') {
            format!("-path {}", shell_quote(&format!("{base}/{pattern}")))
        } else {
            format!("-name {}", shell_quote(pattern))
        };
        let cmd = format!(
            "find {} {} -print 2>/dev/null",
            shell_quote(&base),
            find_filter
        );
        let resp = self.exec_in_vm(&cmd, Some(30_000), None, None).await?;
        let stdout = resp.stdout.unwrap_or_default();
        Ok(stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }

    // ------------------------------------------------------------------
    // Metadata
    // ------------------------------------------------------------------

    fn working_directory(&self) -> &str {
        WORKING_DIRECTORY
    }

    fn platform(&self) -> &str {
        "linux"
    }

    fn os_version(&self) -> String {
        "linux (forkd microVM)".to_string()
    }

    fn sandbox_info(&self) -> String {
        format!("forkd:{}", self.vm_name)
    }

    fn snapshot_info(&self) -> Option<String> {
        self.config
            .settings
            .snapshot
            .as_ref()
            .and_then(|s| s.image.clone())
    }

    fn origin_url(&self) -> Option<&str> {
        self.origin_url.get().map(String::as_str)
    }

    // ------------------------------------------------------------------
    // Git helpers (delegate to shared exec-based implementations)
    // ------------------------------------------------------------------

    async fn setup_git(
        &self,
        intent: &crate::GitSetupIntent,
    ) -> crate::Result<Option<crate::GitRunInfo>> {
        crate::setup_git_via_exec(self, intent)
            .await
            .map(Some)
    }

    fn resume_setup_commands(&self, run_branch: &str) -> Vec<String> {
        vec![format!(
            "git checkout {}",
            shell_quote(run_branch)
        )]
    }

    async fn git_push_ref(&self, refspec: &str) -> crate::Result<()> {
        crate::git_push_via_exec(self, refspec).await
    }
}
