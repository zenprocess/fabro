use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use agent_client_protocol::schema::McpServer;
use agent_client_protocol_tokio::AcpAgent;
use fabro_model::Provider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpCommand {
    display: String,
    program: PathBuf,
    args: Vec<String>,
    env: HashMap<String, String>,
}

impl AcpCommand {
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    #[must_use]
    pub fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    #[must_use]
    pub fn display(&self) -> &str {
        &self.display
    }

    #[must_use]
    pub fn to_shell_command(&self) -> String {
        std::iter::once(self.program.to_string_lossy().into_owned())
            .chain(self.args.iter().cloned())
            .map(|part| fabro_sandbox::shell_quote(&part))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl std::fmt::Display for AcpCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AcpCommandError {
    #[error("acp_command must not be empty")]
    EmptyOverride,
    #[error("only stdio ACP commands are supported")]
    UnsupportedTransport,
    #[error("failed to parse acp_command")]
    Parse(#[source] agent_client_protocol::Error),
}

impl From<agent_client_protocol::Error> for AcpCommandError {
    fn from(error: agent_client_protocol::Error) -> Self {
        Self::Parse(error)
    }
}

#[must_use]
pub fn default_acp_command(provider: Provider) -> AcpCommand {
    match provider {
        Provider::Anthropic => command_from_parts(
            "npx -y @zed-industries/claude-code-acp@latest",
            "npx",
            ["-y", "@zed-industries/claude-code-acp@latest"],
        ),
        Provider::Gemini => command_from_parts(
            "npx -y -- @google/gemini-cli@latest --experimental-acp",
            "npx",
            ["-y", "--", "@google/gemini-cli@latest", "--experimental-acp"],
        ),
        Provider::OpenAi
        | Provider::Kimi
        | Provider::Zai
        | Provider::Minimax
        | Provider::Inception
        | Provider::OpenAiCompatible => command_from_parts(
            "npx -y @zed-industries/codex-acp@latest",
            "npx",
            ["-y", "@zed-industries/codex-acp@latest"],
        ),
    }
}

pub fn resolve_acp_command(
    provider: Provider,
    override_command: Option<&str>,
) -> Result<AcpCommand, AcpCommandError> {
    if let Some(raw) = override_command {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(AcpCommandError::EmptyOverride);
        }
        return parse_acp_command(trimmed);
    }

    Ok(default_acp_command(provider))
}

fn parse_acp_command(raw: &str) -> Result<AcpCommand, AcpCommandError> {
    reject_non_stdio_json_transport(raw)?;

    let agent = AcpAgent::from_str(raw)?;
    let McpServer::Stdio(stdio) = agent.into_server() else {
        return Err(AcpCommandError::UnsupportedTransport);
    };

    Ok(AcpCommand {
        display: raw.to_string(),
        program: stdio.command,
        args: stdio.args,
        env: stdio
            .env
            .into_iter()
            .map(|env| (env.name, env.value))
            .collect(),
    })
}

fn reject_non_stdio_json_transport(raw: &str) -> Result<(), AcpCommandError> {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with('{') {
        return Ok(());
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Ok(());
    };

    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("stdio") | None => Ok(()),
        Some(_) => Err(AcpCommandError::UnsupportedTransport),
    }
}

fn command_from_parts<const N: usize>(
    display: impl Into<String>,
    program: impl Into<PathBuf>,
    args: [&str; N],
) -> AcpCommand {
    AcpCommand {
        display: display.into(),
        program: program.into(),
        args: args.into_iter().map(str::to_string).collect(),
        env: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fabro_model::Provider;

    use super::*;

    #[test]
    fn default_command_for_anthropic_uses_zed_claude_acp() {
        assert_eq!(
            default_acp_command(Provider::Anthropic).to_string(),
            "npx -y @zed-industries/claude-code-acp@latest"
        );
    }

    #[test]
    fn default_command_for_openai_compatible_family_uses_zed_codex_acp() {
        for provider in [
            Provider::OpenAi,
            Provider::Kimi,
            Provider::Zai,
            Provider::Minimax,
            Provider::Inception,
            Provider::OpenAiCompatible,
        ] {
            assert_eq!(
                default_acp_command(provider).to_string(),
                "npx -y @zed-industries/codex-acp@latest"
            );
        }
    }

    #[test]
    fn default_command_for_gemini_uses_experimental_acp() {
        assert_eq!(
            default_acp_command(Provider::Gemini).to_string(),
            "npx -y -- @google/gemini-cli@latest --experimental-acp"
        );
    }

    #[test]
    fn explicit_acp_command_overrides_provider_default() {
        let command = resolve_acp_command(Provider::OpenAi, Some("python fake_agent.py")).unwrap();
        assert_eq!(command.to_string(), "python fake_agent.py");
        assert_eq!(command.program(), Path::new("python"));
        assert_eq!(command.args(), &["fake_agent.py".to_string()]);
    }

    #[test]
    fn blank_acp_command_is_rejected() {
        let err = resolve_acp_command(Provider::OpenAi, Some("   ")).unwrap_err();
        assert!(err.to_string().contains("acp_command must not be empty"));
    }

    #[test]
    fn json_stdio_acp_command_is_supported() {
        let raw = r#"{"type":"stdio","name":"fake","command":"python","args":["fake agent.py"],"env":[{"name":"MODE","value":"test"}]}"#;
        let command = resolve_acp_command(Provider::OpenAi, Some(raw)).unwrap();
        assert_eq!(command.program(), Path::new("python"));
        assert_eq!(command.args(), &["fake agent.py".to_string()]);
        assert_eq!(command.env().get("MODE").map(String::as_str), Some("test"));
    }

    #[test]
    fn non_stdio_acp_command_is_rejected() {
        let raw = r#"{"type":"http","name":"remote","url":"https://example.test/acp"}"#;
        let err = resolve_acp_command(Provider::OpenAi, Some(raw)).unwrap_err();
        assert!(
            err.to_string()
                .contains("only stdio ACP commands are supported")
        );
    }
}
