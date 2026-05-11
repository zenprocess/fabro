mod config;
mod run_tools;
mod server;

use std::path::PathBuf;

pub use config::{config_json, init_agent};
pub use server::start;

#[derive(Debug, Clone)]
pub struct FabroMcpServerSettings {
    pub config:        McpConfigSettings,
    pub server_target: Option<String>,
    pub storage_dir:   PathBuf,
    pub config_path:   PathBuf,
    pub home_dir:      PathBuf,
    pub cwd:           PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct McpConfigSettings {
    pub server:      Option<String>,
    pub storage_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct McpInitSettings {
    pub agent:    McpAgent,
    pub config:   McpConfigSettings,
    pub home_dir: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub enum McpAgent {
    Claude,
    Cursor,
    Windsurf,
}
