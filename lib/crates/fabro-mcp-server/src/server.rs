use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use fabro_client::{
    AuthEntry, AuthStore, Client, Credential, OAuthSession, ServerTarget, TransportConnector,
    apply_bearer_token_auth,
};
use fabro_config::bind::Bind;
use fabro_config::daemon::ServerDaemon;
use fabro_config::{RuntimeDirectory, Storage};
use fabro_util::dev_token;
use fabro_util::version::FABRO_VERSION;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::transport::stdio;
use rmcp::{ErrorData, ServerHandler, serve_server, tool, tool_handler, tool_router};
use tokio::process::Command as TokioCommand;
use tokio::sync::OnceCell;
use tokio::task::yield_now;
use tokio::time::sleep;

use crate::{FabroMcpServerSettings, run_tools};

const CLIENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone)]
pub(crate) struct FabroMcpServer {
    settings:    Arc<FabroMcpServerSettings>,
    client:      Arc<OnceCell<Arc<Client>>>,
    cwd:         PathBuf,
    tool_router: ToolRouter<Self>,
}

pub async fn start(settings: FabroMcpServerSettings) -> Result<()> {
    let server = FabroMcpServer::new(Arc::new(settings));
    let service = serve_server(server, stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FabroMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Use these tools to create, inspect, control, wait for, and read events from Fabro workflow runs.")
    }
}

#[tool_router(router = tool_router)]
impl FabroMcpServer {
    pub(crate) fn new(settings: Arc<FabroMcpServerSettings>) -> Self {
        let cwd = settings.cwd.clone();
        Self {
            settings,
            client: Arc::new(OnceCell::new()),
            cwd,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "fabro_run_create",
        description = "Create one or more Fabro workflow runs, starting them by default."
    )]
    async fn fabro_run_create(
        &self,
        params: Parameters<run_tools::FabroRunCreateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = match run_tools::ValidatedCreateRuns::try_from(params.0) {
            Ok(params) => params,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        let client = match self.client().await {
            Ok(client) => client,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        match run_tools::create_runs(client, &self.cwd, &self.settings.config_path, params).await {
            Ok(result) => run_tools::success_result(&result, run_tools::create_runs_text(&result)),
            Err(err) => Ok(run_tools::error_result(err)),
        }
    }

    #[tool(
        name = "fabro_run_search",
        description = "Search Fabro workflow runs by id, workflow, labels, status, archival state, and creation time."
    )]
    async fn fabro_run_search(
        &self,
        params: Parameters<run_tools::FabroRunSearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = match run_tools::ValidatedSearchRuns::try_from(params.0) {
            Ok(params) => params,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        let client = match self.client().await {
            Ok(client) => client,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        match run_tools::search_runs(client, params).await {
            Ok(result) => run_tools::success_result(&result, run_tools::search_runs_text(&result)),
            Err(err) => Ok(run_tools::error_result(err)),
        }
    }

    #[tool(
        name = "fabro_run_interact",
        description = "Get, start, message, cancel, archive, unarchive, inspect questions, or answer a Fabro run."
    )]
    async fn fabro_run_interact(
        &self,
        params: Parameters<run_tools::FabroRunInteractParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = match run_tools::ValidatedInteractRun::try_from(params.0) {
            Ok(params) => params,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        let client = match self.client().await {
            Ok(client) => client,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        match run_tools::interact_run(client, params).await {
            Ok(result) => run_tools::success_result(&result, run_tools::interact_run_text(&result)),
            Err(err) => Ok(run_tools::error_result(err)),
        }
    }

    #[tool(
        name = "fabro_run_gather",
        description = "Wait for Fabro runs to reach terminal states, returning current state on timeout."
    )]
    async fn fabro_run_gather(
        &self,
        params: Parameters<run_tools::FabroRunGatherParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = match run_tools::ValidatedGatherRuns::try_from(params.0) {
            Ok(params) => params,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        let client = match self.client().await {
            Ok(client) => client,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        match run_tools::gather_runs(client, params).await {
            Ok(result) => run_tools::success_result(&result, run_tools::gather_runs_text(&result)),
            Err(err) => Ok(run_tools::error_result(err)),
        }
    }

    #[tool(
        name = "fabro_run_events",
        description = "List, inspect, or search stored events for a Fabro workflow run."
    )]
    async fn fabro_run_events(
        &self,
        params: Parameters<run_tools::FabroRunEventsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = match run_tools::ValidatedRunEvents::try_from(params.0) {
            Ok(params) => params,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        let client = match self.client().await {
            Ok(client) => client,
            Err(err) => return Ok(run_tools::error_result(err)),
        };
        match run_tools::run_events(client, params).await {
            Ok(result) => run_tools::success_result(&result, run_tools::run_events_text(&result)),
            Err(err) => Ok(run_tools::error_result(err)),
        }
    }

    async fn client(&self) -> Result<Arc<Client>, run_tools::ToolError> {
        self.client
            .get_or_try_init(|| async {
                client_from_settings(&self.settings)
                    .await
                    .map(Arc::new)
                    .map_err(|err| run_tools::ToolError::from_anyhow(&err))
            })
            .await
            .map(Arc::clone)
    }
}

async fn client_from_settings(settings: &FabroMcpServerSettings) -> Result<Client> {
    yield_now().await;
    if let Some(server) = settings.server_target.as_ref() {
        return connect_target(server, settings).await;
    }
    connect_local_server(settings).await
}

async fn connect_target(server: &str, settings: &FabroMcpServerSettings) -> Result<Client> {
    let target: ServerTarget = server.parse()?;
    let auth_store = AuthStore::default();
    let mut credential = resolve_target_credential_with_store(&target, &auth_store)?;
    if credential.is_none() && target.is_unix_socket() {
        let runtime_token_path = Storage::new(&settings.storage_dir)
            .runtime_directory()
            .dev_token_path();
        credential = dev_token::read_dev_token_file(&runtime_token_path).map(Credential::DevToken);
    }
    let oauth_session = refreshable_oauth(&target, &auth_store, credential.as_ref());
    let mut builder = Client::builder()
        .target(target.clone())
        .transport_connector(target_transport_connector(target))
        .request_timeout(CLIENT_REQUEST_TIMEOUT);
    if let Some(credential) = credential {
        builder = builder.credential(credential);
    }
    if let Some(oauth_session) = oauth_session {
        builder = builder.oauth_session(oauth_session);
    }
    builder
        .connect()
        .await
        .context("failed to connect Fabro API")
}

async fn connect_local_server(settings: &FabroMcpServerSettings) -> Result<Client> {
    let bind = ensure_local_server_running(&settings.storage_dir, &settings.config_path).await?;
    match bind {
        Bind::Unix(path) => {
            let token = wait_for_runtime_dev_token(
                &Storage::new(&settings.storage_dir)
                    .runtime_directory()
                    .dev_token_path(),
            )
            .await?;
            let http_client = connect_bind_http_client(&Bind::Unix(path), Some(&token)).await?;
            Client::builder()
                .transport("http://fabro", http_client)
                .request_timeout(CLIENT_REQUEST_TIMEOUT)
                .connect()
                .await
        }
        Bind::Tcp(addr) => {
            let target = ServerTarget::http_url(format!("http://{addr}"))?;
            let auth_store = AuthStore::default();
            let credential = resolve_target_credential_with_store(&target, &auth_store)?;
            let oauth_session = refreshable_oauth(&target, &auth_store, credential.as_ref());
            let mut builder = Client::builder()
                .target(target.clone())
                .transport_connector(target_transport_connector(target))
                .request_timeout(CLIENT_REQUEST_TIMEOUT);
            if let Some(credential) = credential {
                builder = builder.credential(credential);
            }
            if let Some(oauth_session) = oauth_session {
                builder = builder.oauth_session(oauth_session);
            }
            builder.connect().await
        }
    }
}

async fn ensure_local_server_running(storage_dir: &Path, config_path: &Path) -> Result<Bind> {
    let runtime_directory = RuntimeDirectory::new(storage_dir);
    if let Some(existing) = ServerDaemon::load_running(&runtime_directory)? {
        return Ok(existing.bind);
    }

    let exe = std::env::current_exe().context("resolving current fabro executable path")?;
    let status = TokioCommand::new(exe)
        .args(["server", "start", "--no-web", "--storage-dir"])
        .arg(storage_dir)
        .arg("--config")
        .arg(config_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .status()
        .await
        .context("starting local Fabro server")?;
    if !status.success() {
        return Err(anyhow!("fabro server start exited with status {status}"));
    }

    let deadline = std::time::Instant::now() + SERVER_START_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if let Some(running) = ServerDaemon::load_running(&runtime_directory)? {
            return Ok(running.bind);
        }
        sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!(
        "Fabro server started but no active record was found for {}",
        storage_dir.display()
    ))
}

async fn wait_for_runtime_dev_token(path: &Path) -> Result<String> {
    let deadline = std::time::Instant::now() + SERVER_START_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if let Some(token) = dev_token::read_dev_token_file(path) {
            return Ok(token);
        }
        sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!(
        "runtime dev token did not become available at {}",
        path.display()
    ))
}

fn resolve_target_credential_with_store(
    target: &ServerTarget,
    store: &AuthStore,
) -> Result<Option<Credential>> {
    let Some(entry) = store.get(target)? else {
        return Ok(None);
    };
    let now = chrono::Utc::now();
    match entry {
        AuthEntry::DevToken(entry) => Ok(Some(Credential::DevToken(entry.token))),
        AuthEntry::OAuth(entry)
            if entry.access_token_expires_at > now || entry.refresh_token_expires_at > now =>
        {
            Ok(Some(Credential::OAuth(entry)))
        }
        AuthEntry::OAuth(_) => Ok(None),
    }
}

fn refreshable_oauth(
    target: &ServerTarget,
    auth_store: &AuthStore,
    credential: Option<&Credential>,
) -> Option<OAuthSession> {
    matches!(credential, Some(Credential::OAuth(_)))
        .then(|| OAuthSession::new(target.clone(), auth_store.clone()))
}

fn target_transport_connector(target: ServerTarget) -> TransportConnector {
    TransportConnector::new(move |bearer_token| {
        let target = target.clone();
        async move { connect_target_transport(&target, bearer_token.as_deref()) }
    })
}

fn connect_target_transport(
    target: &ServerTarget,
    bearer_token: Option<&str>,
) -> Result<(fabro_http::HttpClient, String)> {
    if let Some(api_url) = target.as_http_url() {
        let mut builder = cli_compatible_http_client_builder();
        if should_bypass_proxy_for_http_target(api_url) {
            builder = builder.no_proxy();
        }
        if let Some(token) = bearer_token {
            builder = apply_bearer_token_auth(builder, token)?;
        }
        return Ok((builder.build()?, api_url.to_string()));
    }

    let Some(path) = target.as_unix_socket_path() else {
        return Err(anyhow!(
            "server target must be an http(s) URL or absolute Unix socket path"
        ));
    };
    let mut builder = fabro_http::HttpClientBuilder::new()
        .unix_socket(path)
        .no_proxy();
    if let Some(token) = bearer_token {
        builder = apply_bearer_token_auth(builder, token)?;
    }
    Ok((builder.build()?, "http://fabro".to_string()))
}

fn cli_compatible_http_client_builder() -> fabro_http::HttpClientBuilder {
    fabro_http::HttpClientBuilder::new().user_agent(format!("fabro-cli/{FABRO_VERSION}"))
}

#[expect(
    clippy::disallowed_types,
    reason = "Proxy bypass classification parses a configured raw API target and does not log credential-bearing URLs."
)]
fn should_bypass_proxy_for_http_target(api_url: &str) -> bool {
    let Ok(url) = fabro_http::Url::parse(api_url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.trim_matches(['[', ']'])
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

async fn connect_bind_http_client(
    bind: &Bind,
    bearer_token: Option<&str>,
) -> Result<fabro_http::HttpClient> {
    let (client, health_url) = match bind {
        Bind::Unix(path) => {
            let mut builder = fabro_http::HttpClientBuilder::new()
                .unix_socket(path)
                .no_proxy();
            if let Some(token) = bearer_token {
                builder = apply_bearer_token_auth(builder, token)?;
            }
            (builder.build()?, "http://fabro/health".to_string())
        }
        Bind::Tcp(addr) => {
            let mut builder = fabro_http::HttpClientBuilder::new().no_proxy();
            if let Some(token) = bearer_token {
                builder = apply_bearer_token_auth(builder, token)?;
            }
            (builder.build()?, format!("http://{addr}/health"))
        }
    };
    let deadline = std::time::Instant::now() + SERVER_START_TIMEOUT;
    let mut last_error = None;
    while std::time::Instant::now() < deadline {
        match client.get(&health_url).send().await {
            Ok(response) if response.status().is_success() => return Ok(client),
            Ok(response) => last_error = Some(anyhow!("health returned {}", response.status())),
            Err(err) => last_error = Some(anyhow!(err)),
        }
        sleep(Duration::from_millis(50)).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow!("Fabro server did not become ready in time")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_http_proxy_bypass_matches_cli_for_local_targets() {
        assert!(should_bypass_proxy_for_http_target(
            "http://localhost:3000/api/v1"
        ));
        assert!(should_bypass_proxy_for_http_target(
            "http://127.0.0.1:3000/api/v1"
        ));
        assert!(should_bypass_proxy_for_http_target(
            "http://[::1]:3000/api/v1"
        ));
    }

    #[test]
    fn explicit_http_proxy_bypass_matches_cli_for_remote_targets() {
        assert!(!should_bypass_proxy_for_http_target(
            "https://fabro.example.test/api/v1"
        ));
        assert!(!should_bypass_proxy_for_http_target(
            "http://192.0.2.44:3000/api/v1"
        ));
    }
}
