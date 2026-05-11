use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use fabro_client::{
    AuthEntry, AuthStore, Client, Credential, ServerTarget, TransportConnector,
    apply_bearer_token_auth,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::transport::stdio;
use rmcp::{ErrorData, ServerHandler, serve_server, tool, tool_handler, tool_router};
use tokio::sync::OnceCell;
use tokio::task::yield_now;

use crate::{McpServerSettings, run_tools};

#[derive(Clone)]
pub(crate) struct FabroMcpServer {
    settings:    Arc<McpServerSettings>,
    client:      Arc<OnceCell<Arc<Client>>>,
    cwd:         PathBuf,
    tool_router: ToolRouter<Self>,
}

pub async fn start(settings: McpServerSettings) -> Result<()> {
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
    pub(crate) fn new(settings: Arc<McpServerSettings>) -> Self {
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
        match run_tools::create_runs(client, &self.cwd, params).await {
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

async fn client_from_settings(settings: &McpServerSettings) -> Result<Client> {
    yield_now().await;
    let Some(server) = settings.config.server.as_ref() else {
        return Err(anyhow!(
            "fabro mcp start requires --server for run tools in this release"
        ));
    };
    let target: ServerTarget = server.parse()?;
    let credential = AuthStore::new(settings.home_dir.join(".fabro").join("auth.json"))
        .get(&target)?
        .map(credential_from_auth_entry);
    let mut builder = Client::builder()
        .target(target.clone())
        .transport_connector(target_transport_connector(target))
        .request_timeout(std::time::Duration::from_secs(30));
    if let Some(credential) = credential {
        builder = builder.credential(credential);
    }
    builder
        .connect()
        .await
        .context("failed to connect Fabro API")
}

fn credential_from_auth_entry(entry: AuthEntry) -> Credential {
    match entry {
        AuthEntry::OAuth(entry) => Credential::OAuth(entry),
        AuthEntry::DevToken(entry) => Credential::DevToken(entry.token),
    }
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
        let mut builder = fabro_http::HttpClientBuilder::new().no_proxy();
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
