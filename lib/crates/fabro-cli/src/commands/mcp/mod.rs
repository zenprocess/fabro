use std::fmt::Write as _;

use anyhow::{Context as _, Result};

use crate::args::{McpAgent, McpCommand, McpNamespace, ServerConnectionArgs};
use crate::command_context::CommandContext;
use crate::user_config;

pub(crate) async fn dispatch(ns: McpNamespace, base_ctx: &CommandContext) -> Result<()> {
    match ns.command {
        McpCommand::Start(args) => {
            fabro_mcp_server::start(server_settings(base_ctx, &args.connection)?).await
        }
        McpCommand::Config(args) => {
            let json = fabro_mcp_server::config_json(&config_settings(&args.connection))?;
            let _ = write!(base_ctx.printer().stdout_important(), "{json}");
            Ok(())
        }
        McpCommand::Init(args) => {
            fabro_mcp_server::init_agent(&init_settings(args.agent, &args.connection)?)?;
            Ok(())
        }
    }
}

fn server_settings(
    base_ctx: &CommandContext,
    connection: &ServerConnectionArgs,
) -> Result<fabro_mcp_server::FabroMcpServerSettings> {
    let connection_ctx = base_ctx.with_connection(connection)?;
    let server_target = user_config::resolve_nondefault_server_target(
        &connection.target,
        connection_ctx.user_settings(),
    )?
    .map(|target| {
        target
            .as_unix_socket_path()
            .map_or_else(|| target.to_string(), |path| path.display().to_string())
    });
    Ok(fabro_mcp_server::FabroMcpServerSettings {
        config: config_settings(connection),
        server_target,
        storage_dir: connection_ctx.storage_dir().to_path_buf(),
        config_path: connection_ctx.base_config_path().to_path_buf(),
        home_dir: home_dir()?,
        cwd: base_ctx.cwd().to_path_buf(),
    })
}

fn init_settings(
    agent: McpAgent,
    connection: &ServerConnectionArgs,
) -> Result<fabro_mcp_server::McpInitSettings> {
    Ok(fabro_mcp_server::McpInitSettings {
        agent:    McpAgentForServer(agent).into(),
        config:   config_settings(connection),
        home_dir: home_dir()?,
    })
}

fn config_settings(connection: &ServerConnectionArgs) -> fabro_mcp_server::McpConfigSettings {
    fabro_mcp_server::McpConfigSettings {
        server:      connection.target.server.clone(),
        storage_dir: connection.storage_dir.clone_path(),
    }
}

fn home_dir() -> Result<std::path::PathBuf> {
    dirs::home_dir().context("failed to resolve home directory for MCP config")
}

struct McpAgentForServer(McpAgent);

impl From<McpAgentForServer> for fabro_mcp_server::McpAgent {
    fn from(value: McpAgentForServer) -> Self {
        match value.0 {
            McpAgent::Claude => Self::Claude,
            McpAgent::Cursor => Self::Cursor,
            McpAgent::Windsurf => Self::Windsurf,
        }
    }
}
