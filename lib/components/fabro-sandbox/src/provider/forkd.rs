use std::time::Duration;

use async_trait::async_trait;
use fabro_types::{SandboxInfo, SandboxProviderKind};
use tokio::time::sleep;

use super::{SandboxCreateSpec, SandboxProvider};
use crate::forkd::{ForkdConfig, ForkdSandbox};
use crate::{Sandbox, details};

/// Retry limit for transient HTTP failures (5xx / connect) in provider calls.
const PROVIDER_RETRY_LIMIT: u32 = 3;
const PROVIDER_RETRY_INITIAL_BACKOFF: Duration = Duration::from_millis(250);

/// A [`SandboxProvider`] that creates Firecracker microVMs via a forkd
/// controller.  The controller URL and bearer token are resolved once at
/// construction time from `FORKD_URL` / `FORKD_TOKEN`; they never appear in
/// per-run specs.
#[derive(Clone)]
pub struct ForkdSandboxProvider {
    config: ForkdConfig,
}

impl ForkdSandboxProvider {
    /// Build a provider from an already-resolved [`ForkdConfig`].
    pub fn new(config: ForkdConfig) -> Self {
        Self { config }
    }

    /// Build a provider by reading `FORKD_URL`, `FORKD_TOKEN`, and
    /// `FORKD_SNAPSHOT_TAG` from the process environment (the same
    /// resolution that [`ForkdConfig::from_env`] and
    /// [`crate::from_environment::forkd_config_from_environment`] perform).
    pub fn from_env() -> Self {
        Self::new(ForkdConfig::from_env())
    }

    fn http_client(&self) -> crate::Result<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| crate::Error::context("Failed to build HTTP client for forkd", e))
    }

    fn is_retryable_status(status: reqwest::StatusCode) -> bool {
        status.is_server_error()
    }
}

#[async_trait]
impl SandboxProvider for ForkdSandboxProvider {
    fn kind(&self) -> SandboxProviderKind {
        SandboxProviderKind::Forkd
    }

    async fn list(&self) -> crate::Result<Vec<SandboxInfo>> {
        let client = self.http_client()?;
        let url = format!("{}/v1/sandboxes", self.config.forkd_url);

        let mut backoff = PROVIDER_RETRY_INITIAL_BACKOFF;
        let mut attempt = 0u32;
        let resp = loop {
            let result = client
                .get(&url)
                .bearer_auth(&self.config.forkd_token)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => break resp,
                Ok(resp)
                    if Self::is_retryable_status(resp.status())
                        && attempt < PROVIDER_RETRY_LIMIT =>
                {
                    let status = resp.status();
                    tracing::warn!(
                        attempt,
                        status = status.as_u16(),
                        "forkd list transient error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(crate::Error::message(format!(
                        "forkd list VMs returned {status}: {body}"
                    )));
                }
                Err(e) if e.is_connect() && attempt < PROVIDER_RETRY_LIMIT => {
                    tracing::warn!(attempt, error = %e, "forkd list connect error; retrying");
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Err(e) => {
                    return Err(crate::Error::context("Failed to list forkd VMs", e));
                }
            }
        };

        let vms: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::Error::context("Failed to parse forkd VM list", e))?;

        // forkd 0.5.2 returns a top-level JSON array of sandbox objects, each
        // carrying an "id" field.
        let arr = vms.as_array().cloned().unwrap_or_default();

        let sandboxes = arr
            .into_iter()
            .filter_map(|vm| {
                let id = vm.get("id")?.as_str()?.to_string();
                Some(details::forkd::forkd_info_from_name(&id))
            })
            .collect();

        Ok(sandboxes)
    }

    async fn get(&self, id: &str) -> crate::Result<Option<SandboxInfo>> {
        let client = self.http_client()?;
        let url = format!("{}/v1/sandboxes/{}", self.config.forkd_url, id);

        let mut backoff = PROVIDER_RETRY_INITIAL_BACKOFF;
        let mut attempt = 0u32;
        loop {
            let result = client
                .get(&url)
                .bearer_auth(&self.config.forkd_token)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => return Ok(None),
                Ok(resp) if resp.status().is_success() => {
                    return Ok(Some(details::forkd::forkd_info_from_name(id)));
                }
                Ok(resp)
                    if Self::is_retryable_status(resp.status())
                        && attempt < PROVIDER_RETRY_LIMIT =>
                {
                    let status = resp.status();
                    tracing::warn!(
                        attempt,
                        status = status.as_u16(),
                        "forkd get transient error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(crate::Error::message(format!(
                        "forkd get VM '{id}' returned {status}: {body}"
                    )));
                }
                Err(e) if e.is_connect() && attempt < PROVIDER_RETRY_LIMIT => {
                    tracing::warn!(attempt, error = %e, "forkd get connect error; retrying");
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Err(e) => {
                    return Err(crate::Error::context(
                        format!("Failed to get forkd VM '{id}'"),
                        e,
                    ));
                }
            }
        }
    }

    async fn create(&self, spec: SandboxCreateSpec) -> crate::Result<SandboxInfo> {
        let SandboxCreateSpec::Forkd {
            config,
            run_id,
            clone_origin_url,
            clone_branch,
        } = spec
        else {
            return Err(crate::Error::message(
                "ForkdSandboxProvider requires a SandboxCreateSpec::Forkd variant",
            ));
        };

        let merged_config = ForkdConfig {
            forkd_url:   self.config.forkd_url.clone(),
            forkd_token: self.config.forkd_token.clone(),
            settings:    *config,
        };

        let sandbox = ForkdSandbox::new(merged_config, run_id, clone_origin_url, clone_branch);
        // Provision the microVM first; forkd assigns the sandbox id, which we
        // then report. Without initialize() the VM never exists and all
        // subsequent operations on the returned SandboxInfo would fail.
        sandbox.initialize().await?;
        let id = sandbox
            .sandbox_id()
            .ok_or_else(|| crate::Error::message("forkd sandbox id missing after initialize"))?;
        Ok(details::forkd::forkd_info_from_name(id))
    }

    async fn delete(&self, id: &str) -> crate::Result<()> {
        let client = self.http_client()?;
        let url = format!("{}/v1/sandboxes/{}", self.config.forkd_url, id);

        let mut backoff = PROVIDER_RETRY_INITIAL_BACKOFF;
        let mut attempt = 0u32;
        loop {
            let result = client
                .delete(&url)
                .bearer_auth(&self.config.forkd_token)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                    // Already gone — treat as success.
                    return Ok(());
                }
                Ok(resp) if resp.status().is_success() => return Ok(()),
                Ok(resp)
                    if Self::is_retryable_status(resp.status())
                        && attempt < PROVIDER_RETRY_LIMIT =>
                {
                    let status = resp.status();
                    tracing::warn!(
                        attempt,
                        status = status.as_u16(),
                        "forkd delete transient error; retrying"
                    );
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(crate::Error::message(format!(
                        "forkd delete VM '{id}' returned {status}: {body}"
                    )));
                }
                Err(e) if e.is_connect() && attempt < PROVIDER_RETRY_LIMIT => {
                    tracing::warn!(attempt, error = %e, "forkd delete connect error; retrying");
                    attempt += 1;
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                Err(e) => {
                    return Err(crate::Error::context(
                        format!("Failed to delete forkd VM '{id}'"),
                        e,
                    ));
                }
            }
        }
    }
}

#[cfg(all(test, feature = "forkd"))]
mod tests {
    //! Hermetic tests for the forkd provider.
    //!
    //! Every HTTP call is answered by a `httpmock` server on localhost — no
    //! network beyond the loopback, no real forkd controller, no real tokens
    //! (dummy bearer strings only). The retry/backoff assertions pin the
    //! behavior referenced by the upstream issue (issue #125 in the
    //! `zenprocess/ao-company` issue tracker).

    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    use httpmock::Method;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::config::ForkdSettings;

    /// Serialize the env-mutating tests so that concurrent reads from
    /// `from_env()` see a deterministic environment. `std::env::set_var` is
    /// not thread-safe, so the four `from_env_*` tests coordinate here.
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn provider_for(base_url: &str) -> ForkdSandboxProvider {
        ForkdSandboxProvider::new(ForkdConfig {
            forkd_url:   base_url.to_string(),
            forkd_token: "forkd-test-token".to_string(),
            settings:    ForkdSettings::default(),
        })
    }

    /// Spin up a localhost TCP listener that returns a scripted sequence of
    /// status codes for `GET /v1/sandboxes`, and a JSON 200 body on the final
    /// success attempt. Returns the http://127.0.0.1:port base URL, a shared
    /// attempt counter, and a `JoinHandle` that is held for the lifetime of
    /// the test so the listener is not dropped prematurely.
    ///
    /// Used to express the retry-success contract directly — httpmock 0.8
    /// cannot sequence per-call responses on a single path.
    async fn scripted_list_server(
        statuses: Vec<u16>,
        success_body: &'static [u8],
    ) -> (String, Arc<AtomicU32>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_inner = attempts.clone();
        let handle = tokio::spawn(async move {
            // Accept exactly `statuses.len()` connections — one per provider
            // attempt — so the test is hermetic and does not hang.
            for status in statuses {
                let (mut stream, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                attempts_inner.fetch_add(1, Ordering::SeqCst);
                // Drain the request bytes so the server-side kernel buffer
                // does not stall. We don't care about the request content —
                // only the status / body we write back.
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let (status_line, body): (&str, &[u8]) = if status < 400 {
                    ("200 OK", success_body)
                } else {
                    (
                        match status {
                            500 => "500 Internal Server Error",
                            502 => "502 Bad Gateway",
                            503 => "503 Service Unavailable",
                            504 => "504 Gateway Timeout",
                            _ => "500 Internal Server Error",
                        },
                        b"",
                    )
                };
                let len = body.len();
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                if !body.is_empty() {
                    let _ = stream.write_all(body).await;
                }
                let _ = stream.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{port}"), attempts, handle)
    }

    // ------------------------------------------------------------------
    // list()
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn list_returns_sandbox_infos_on_200() {
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/v1/sandboxes");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(serde_json::json!([
                        { "id": "vm-1" },
                        { "id": "vm-2" }
                    ]));
            })
            .await;

        let infos = provider_for(&server.base_url()).list().await.expect("list");

        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].id, "vm-1");
        assert_eq!(infos[0].provider, SandboxProviderKind::Forkd);
        assert_eq!(infos[1].id, "vm-2");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn list_retries_5xx_then_succeeds_with_exponential_backoff() {
        // Commit-litmus: this test FAILS when PROVIDER_RETRY_LIMIT is set to 0
        // (no retries) — the first 500 short-circuits to a typed error, the
        // 200 terminal response is never read. With the limit restored the
        // third attempt sees the 200 and the test passes.
        //
        // httpmock cannot sequence per-call responses on a single path, so
        // the test stands up a small TcpListener that responds 500, 500, 200
        // and counts attempts. The exponential-backoff timing budget keeps
        // this well within the 30s reqwest client timeout configured by the
        // provider.
        let (base_url, attempts, _server) =
            scripted_list_server(vec![500, 500, 200], b"[{\"id\":\"vm-flaky\"}]").await;

        let provider = provider_for(&base_url);
        let infos = provider.list().await.expect("3rd attempt must succeed");

        assert_eq!(attempts.load(Ordering::SeqCst), 3, "expected 3 attempts");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "vm-flaky");
    }

    #[tokio::test]
    async fn list_succeeds_when_first_response_is_200() {
        // Sanity test: a non-failing first call does not retry. Equivalent to
        // list_returns_sandbox_infos_on_200 above, kept separate so failure
        // isolation is obvious in CI logs.
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/v1/sandboxes");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(serde_json::json!([]));
            })
            .await;

        let infos = provider_for(&server.base_url()).list().await.expect("list");
        assert!(infos.is_empty());
        mock.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn list_5xx_until_retry_limit_yields_typed_error() {
        let server = httpmock::MockServer::start_async().await;
        // Register the same response for every hit. httpmock returns the same
        // mock regardless of call count — so 3 initial + 3 retries = at most
        // 4 hits (retry limit is 3, meaning 3 retries beyond the initial call).
        // The mock receives every call.
        let m = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/v1/sandboxes");
                then.status(500).body("down");
            })
            .await;

        let result = provider_for(&server.base_url()).list().await;
        let err = result.expect_err("500 storm must surface a typed error");
        let msg = format!("{err}");
        assert!(
            msg.contains("returned 500") || msg.contains("500"),
            "expected a typed 500 error, got: {msg}"
        );
        // 1 initial + 3 retries = 4 attempts total.
        m.assert_calls_async(4).await;
    }

    #[tokio::test]
    async fn list_401_fails_fast_without_retries() {
        let server = httpmock::MockServer::start_async().await;
        let m = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/v1/sandboxes");
                then.status(401).body("unauthorized");
            })
            .await;

        let result = provider_for(&server.base_url()).list().await;
        let err = result.expect_err("401 must surface a typed error");
        let msg = format!("{err}");
        assert!(msg.contains("401"), "expected 401 in message, got: {msg}");
        // 4xx is not retryable — exactly one attempt.
        m.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn list_connect_error_after_retries_yields_typed_error() {
        // Bind a listener, capture its port, then drop the listener so the
        // port stops accepting connections. Every reqwest attempt hits a
        // connect-refused error; after PROVIDER_RETRY_LIMIT retries the
        // provider surfaces a typed error rather than panicking or
        // returning silently.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let provider = provider_for(&format!("http://127.0.0.1:{port}"));
        let result = provider.list().await;
        let err = result.expect_err("connect-refused must surface a typed error");
        let msg = format!("{err}");
        assert!(
            msg.contains("Failed to list forkd VMs")
                || msg.to_lowercase().contains("connection refused")
                || msg.to_lowercase().contains("connect"),
            "expected a typed connect error, got: {msg}"
        );
    }

    // ------------------------------------------------------------------
    // get()
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn get_returns_none_on_404() {
        let server = httpmock::MockServer::start_async().await;
        let m = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/v1/sandboxes/missing");
                then.status(404);
            })
            .await;

        let info = provider_for(&server.base_url())
            .get("missing")
            .await
            .expect("404 must be Ok(None), not Err");
        assert!(info.is_none(), "404 must map to Ok(None)");
        m.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn get_returns_info_on_200() {
        let server = httpmock::MockServer::start_async().await;
        let m = server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/v1/sandboxes/vm-alive");
                then.status(200);
            })
            .await;

        let info = provider_for(&server.base_url())
            .get("vm-alive")
            .await
            .expect("200 must be Ok(Some)")
            .expect("must be Some");
        assert_eq!(info.id, "vm-alive");
        assert_eq!(info.provider, SandboxProviderKind::Forkd);
        m.assert_calls_async(1).await;
    }

    // ------------------------------------------------------------------
    // create()
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn create_rejects_non_forkd_spec_with_typed_error() {
        // A spec whose variant is not the forkd variant must be rejected
        // without making any HTTP call, rather than silently mapping or
        // panicking.
        let server = httpmock::MockServer::start_async().await;
        let m = server
            .mock_async(|when, _| {
                when.method(Method::POST);
            })
            .await;

        let provider = provider_for(&server.base_url());
        let result = provider.create(SandboxCreateSpec::Local).await;
        let err = result.expect_err("non-Forkd spec must be rejected");
        let msg = format!("{err}");
        assert!(
            msg.contains("ForkdSandboxProvider requires a SandboxCreateSpec::Forkd variant"),
            "expected typed variant-mismatch error, got: {msg}"
        );
        // No HTTP call should have been issued.
        m.assert_calls_async(0).await;
    }

    #[tokio::test]
    async fn create_reports_server_assigned_id_and_provider_kind() {
        // The provider's create() calls ForkdSandbox::initialize(), which
        // POSTs /v1/sandboxes. We mock that endpoint to return a
        // server-assigned id; the returned SandboxInfo must echo that id.
        // skip_clone=true keeps the test free of any git-repo setup.
        let server = httpmock::MockServer::start_async().await;
        let create_mock = server
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/sandboxes")
                    .header("authorization", "Bearer forkd-test-token")
                    .header("content-type", "application/json");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(serde_json::json!([{
                        "id": "vm-from-server",
                        "snapshot_tag": "zen-gate-base"
                    }]));
            })
            .await;

        let settings = ForkdSettings {
            snapshot_tag: "zen-gate-base".to_string(),
            skip_clone: true,
            ..ForkdSettings::default()
        };
        let provider = ForkdSandboxProvider::new(ForkdConfig {
            forkd_url: server.base_url(),
            forkd_token: "forkd-test-token".to_string(),
            settings,
        });
        let info = provider
            .create(SandboxCreateSpec::Forkd {
                config:           Box::new(ForkdSettings {
                    snapshot_tag: "zen-gate-base".to_string(),
                    skip_clone: true,
                    ..ForkdSettings::default()
                }),
                run_id:           None,
                clone_origin_url: None,
                clone_branch:     None,
            })
            .await
            .expect("create must succeed against mocked forkd");

        assert_eq!(info.id, "vm-from-server");
        assert_eq!(info.provider, SandboxProviderKind::Forkd);
        create_mock.assert_calls_async(1).await;
    }

    // ------------------------------------------------------------------
    // delete()
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn delete_200_returns_ok() {
        let server = httpmock::MockServer::start_async().await;
        let m = server
            .mock_async(|when, then| {
                when.method(Method::DELETE).path("/v1/sandboxes/vm-1");
                then.status(204);
            })
            .await;

        provider_for(&server.base_url())
            .delete("vm-1")
            .await
            .expect("204 must succeed");
        m.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn delete_404_is_treated_as_idempotent_success() {
        let server = httpmock::MockServer::start_async().await;
        let m = server
            .mock_async(|when, then| {
                when.method(Method::DELETE).path("/v1/sandboxes/ghost");
                then.status(404);
            })
            .await;

        provider_for(&server.base_url())
            .delete("ghost")
            .await
            .expect("404 must be idempotent Ok(())");
        m.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn delete_401_fails_fast_without_retries() {
        let server = httpmock::MockServer::start_async().await;
        let m = server
            .mock_async(|when, then| {
                when.method(Method::DELETE).path("/v1/sandboxes/vm-1");
                then.status(401).body("unauthorized");
            })
            .await;

        let result = provider_for(&server.base_url()).delete("vm-1").await;
        let err = result.expect_err("401 delete must fail");
        let msg = format!("{err}");
        assert!(msg.contains("401"), "expected 401 in message, got: {msg}");
        // No retries on 4xx — exactly one attempt.
        m.assert_calls_async(1).await;
    }

    // ------------------------------------------------------------------
    // from_env()
    // ------------------------------------------------------------------

    #[test]
    fn from_env_uses_defaults_when_no_overrides_set() {
        let _guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        // Clear the relevant env vars so the default fallback path runs.
        std::env::remove_var("FORKD_URL");
        std::env::remove_var("FORKD_TOKEN");
        std::env::remove_var("FORKD_SNAPSHOT_TAG");

        let cfg = ForkdConfig::from_env();
        assert_eq!(cfg.forkd_url, "http://127.0.0.1:8889");
        assert_eq!(cfg.forkd_token, "forkd-local-token");
        assert_eq!(
            cfg.settings.snapshot_tag,
            crate::forkd::DEFAULT_SNAPSHOT_TAG,
            "default snapshot tag must equal the documented default ({})",
            crate::forkd::DEFAULT_SNAPSHOT_TAG
        );
    }

    #[test]
    fn from_env_honors_forkd_url_token_snapshot_tag() {
        let _guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("FORKD_URL", "http://example.test:9999");
        std::env::set_var("FORKD_TOKEN", "override-token-value");
        std::env::set_var("FORKD_SNAPSHOT_TAG", "custom-snapshot");

        let cfg = ForkdConfig::from_env();
        assert_eq!(cfg.forkd_url, "http://example.test:9999");
        assert_eq!(cfg.forkd_token, "override-token-value");
        assert_eq!(cfg.settings.snapshot_tag, "custom-snapshot");

        std::env::remove_var("FORKD_URL");
        std::env::remove_var("FORKD_TOKEN");
        std::env::remove_var("FORKD_SNAPSHOT_TAG");
    }

    #[test]
    fn from_env_only_snapshot_tag_overrides_default() {
        let _guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("FORKD_URL");
        std::env::remove_var("FORKD_TOKEN");
        std::env::set_var("FORKD_SNAPSHOT_TAG", "only-tag-set");

        let cfg = ForkdConfig::from_env();
        assert_eq!(cfg.forkd_url, "http://127.0.0.1:8889");
        assert_eq!(cfg.forkd_token, "forkd-local-token");
        assert_eq!(cfg.settings.snapshot_tag, "only-tag-set");

        std::env::remove_var("FORKD_SNAPSHOT_TAG");
    }
}
