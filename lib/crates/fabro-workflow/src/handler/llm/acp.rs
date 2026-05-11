//! Workflow adapter for ACP-backed LLM stages.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use fabro_acp::{AcpError, AcpRunRequest, default_acp_command, resolve_acp_command};
use fabro_agent::{Sandbox, StaticEnvProvider, ToolEnvProvider};
use fabro_auth::{CliAgentKind, CredentialResolver, CredentialUsage, ResolvedCredential};
use fabro_graphviz::graph::Node;
use fabro_model::Provider;
use fabro_util::time::elapsed_ms;
use tokio_util::sync::CancellationToken;

use super::super::agent::{CodergenBackend, CodergenResult};
use super::cli::{AgentCli, process_env_var};
use super::{changed_files, node_runtime};
use crate::context::Context;
use crate::error::Error;
use crate::event::{Emitter, Event, RunNoticeCode, RunNoticeLevel, StageScope};

pub struct AgentAcpBackend {
    model: String,
    provider: Provider,
    tool_env: Option<Arc<dyn ToolEnvProvider>>,
    github_token_refresh_managed: bool,
    resolver: Option<CredentialResolver>,
}

impl AgentAcpBackend {
    #[must_use]
    pub fn new(model: String, provider: Provider, resolver: CredentialResolver) -> Self {
        Self {
            model,
            provider,
            tool_env: None,
            github_token_refresh_managed: false,
            resolver: Some(resolver),
        }
    }

    #[must_use]
    pub fn new_from_env(model: String, provider: Provider) -> Self {
        Self {
            model,
            provider,
            tool_env: None,
            github_token_refresh_managed: false,
            resolver: None,
        }
    }

    #[must_use]
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.tool_env = Some(Arc::new(StaticEnvProvider(env)));
        self
    }

    #[must_use]
    pub fn with_tool_env_provider(
        mut self,
        provider: Arc<dyn ToolEnvProvider>,
        github_token_refresh_managed: bool,
    ) -> Self {
        self.tool_env = Some(provider);
        self.github_token_refresh_managed = github_token_refresh_managed;
        self
    }

    async fn run_turn(
        &self,
        node: &Node,
        prompt: String,
        emitter: &Arc<Emitter>,
        stage_scope: &StageScope,
        sandbox: &Arc<dyn Sandbox>,
        cancel_token: CancellationToken,
    ) -> Result<CodergenResult, Error> {
        let files_before = changed_files::detect_changed_files(sandbox).await;
        let model = node.model().unwrap_or(&self.model);
        let provider = node
            .provider()
            .and_then(|value| value.parse::<Provider>().ok())
            .unwrap_or(self.provider);
        let explicit_command = node.acp_command();
        let command = resolve_acp_command(provider, explicit_command)
            .map_err(|err| Error::handler_with_source("Failed to resolve ACP command", &err))?;

        if explicit_command.is_none()
            && command.program() == default_acp_command(provider).program()
        {
            node_runtime::ensure_node_runtime(sandbox, &cancel_token).await?;
        }

        let launch_env = self
            .launch_env(provider, emitter, sandbox, &cancel_token)
            .await?;
        let on_activity = {
            let emitter = Arc::clone(emitter);
            Arc::new(move || emitter.touch()) as Arc<dyn Fn() + Send + Sync>
        };

        let command_display = command.to_string();
        emitter.emit_scoped(
            &Event::AgentAcpStarted {
                node_id:  node.id.clone(),
                visit:    stage_scope.visit,
                mode:     "acp".to_string(),
                provider: provider.to_string(),
                model:    model.to_string(),
                command:  command_display,
            },
            stage_scope,
        );

        let launch_start = std::time::Instant::now();
        let result = match fabro_acp::run_acp_turn(AcpRunRequest {
            command,
            prompt,
            cwd: sandbox.working_directory().to_string(),
            timeout_ms: node.timeout().map(crate::millis_u64),
            env: launch_env,
            sandbox: Arc::clone(sandbox),
            cancel_token: cancel_token.child_token(),
            on_activity: Some(on_activity),
        })
        .await
        {
            Ok(result) => {
                emitter.emit_scoped(
                    &Event::AgentAcpCompleted {
                        node_id:     node.id.clone(),
                        stdout:      result.text.clone(),
                        stderr:      result.stderr.clone(),
                        stop_reason: stop_reason_to_string(&result.stop_reason),
                        duration_ms: result.duration_ms,
                    },
                    stage_scope,
                );
                result
            }
            Err(AcpError::Cancelled) => {
                emitter.emit_scoped(
                    &Event::AgentAcpCancelled {
                        node_id:     node.id.clone(),
                        stdout:      String::new(),
                        stderr:      String::new(),
                        duration_ms: elapsed_ms(launch_start),
                    },
                    stage_scope,
                );
                return Err(Error::Cancelled);
            }
            Err(AcpError::TimedOut { stderr }) => {
                emitter.emit_scoped(
                    &Event::AgentAcpTimedOut {
                        node_id:     node.id.clone(),
                        stdout:      String::new(),
                        stderr:      stderr.clone(),
                        duration_ms: elapsed_ms(launch_start),
                    },
                    stage_scope,
                );
                return Err(acp_error_to_workflow(AcpError::TimedOut { stderr }));
            }
            Err(AcpError::StopReason { stop_reason, text }) => {
                emitter.emit_scoped(
                    &Event::AgentAcpCompleted {
                        node_id:     node.id.clone(),
                        stdout:      text.clone(),
                        stderr:      String::new(),
                        stop_reason: stop_reason.clone(),
                        duration_ms: elapsed_ms(launch_start),
                    },
                    stage_scope,
                );
                return Err(acp_error_to_workflow(AcpError::StopReason {
                    stop_reason,
                    text,
                }));
            }
            Err(error) => return Err(acp_error_to_workflow(error)),
        };

        let (files_touched, last_file_touched) =
            changed_files::files_touched_since(sandbox, &files_before).await;

        Ok(CodergenResult::Text {
            text: result.text,
            usage: None,
            files_touched,
            last_file_touched,
        })
    }

    async fn launch_env(
        &self,
        provider: Provider,
        emitter: &Arc<Emitter>,
        sandbox: &Arc<dyn Sandbox>,
        cancel_token: &CancellationToken,
    ) -> Result<HashMap<String, String>, Error> {
        let cli_agent = match AgentCli::for_provider(provider) {
            AgentCli::Claude => CliAgentKind::Claude,
            AgentCli::Codex => CliAgentKind::Codex,
            AgentCli::Gemini => CliAgentKind::Gemini,
        };
        let mut launch_env = if let Some(resolver) = &self.resolver {
            let resolved = resolver
                .resolve(provider, CredentialUsage::CliAgent(cli_agent))
                .await
                .map_err(|err| {
                    Error::handler_with_source("Failed to resolve ACP credential", &err)
                })?;
            let ResolvedCredential::Cli(cli_credential) = resolved else {
                return Err(Error::handler("Expected CLI credential".to_string()));
            };
            if let Some(login_cmd) = &cli_credential.login_command {
                let login_result = sandbox
                    .exec_command(
                        login_cmd,
                        30_000,
                        None,
                        None,
                        Some(cancel_token.child_token()),
                    )
                    .await
                    .map_err(|err| {
                        Error::handler_with_source("ACP credential login failed", &err)
                    })?;
                if !login_result.is_success() {
                    tracing::warn!(
                        exit_code = login_result.display_exit_code(),
                        "ACP credential login failed: {}",
                        login_result.stderr
                    );
                }
            }
            cli_credential.env_vars
        } else {
            let mut env = HashMap::new();
            for name in provider.api_key_env_vars() {
                if let Some(value) = process_env_var(name) {
                    env.insert((*name).to_string(), value);
                }
            }
            env
        };

        if let Some(provider) = &self.tool_env {
            if self.github_token_refresh_managed {
                emitter.notice(
                    RunNoticeLevel::Info,
                    RunNoticeCode::GithubTokenRefreshLimited,
                    "ACP agent stages receive GitHub tokens at process launch; stages running \
                     beyond token expiry may need to be retried.",
                );
            }
            let tool_env = provider.resolve().await.map_err(|err| {
                Error::handler_with_anyhow("Failed to resolve ACP agent env", &err)
            })?;
            launch_env.extend(tool_env);
        }

        Ok(launch_env)
    }
}

#[async_trait]
impl CodergenBackend for AgentAcpBackend {
    async fn run(
        &self,
        node: &Node,
        prompt: &str,
        context: &Context,
        _thread_id: Option<&str>,
        emitter: &Arc<Emitter>,
        sandbox: &Arc<dyn Sandbox>,
        _tool_hooks: Option<Arc<dyn fabro_agent::ToolHookCallback>>,
        cancel_token: CancellationToken,
    ) -> Result<CodergenResult, Error> {
        let stage_scope = StageScope::for_handler(context, &node.id);
        self.run_turn(
            node,
            prompt.to_string(),
            emitter,
            &stage_scope,
            sandbox,
            cancel_token,
        )
        .await
    }

    async fn one_shot(
        &self,
        node: &Node,
        prompt: &str,
        system_prompt: Option<&str>,
        emitter: &Arc<Emitter>,
        stage_scope: &StageScope,
        sandbox: &Arc<dyn Sandbox>,
        cancel_token: CancellationToken,
    ) -> Result<CodergenResult, Error> {
        let prompt = match system_prompt.filter(|prompt| !prompt.is_empty()) {
            Some(system_prompt) => format!("System:\n{system_prompt}\n\nUser:\n{prompt}"),
            None => prompt.to_string(),
        };
        self.run_turn(node, prompt, emitter, stage_scope, sandbox, cancel_token)
            .await
    }
}

fn stop_reason_to_string(stop_reason: &(impl serde::Serialize + std::fmt::Debug)) -> String {
    serde_json::to_value(stop_reason)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{stop_reason:?}"))
}

fn acp_error_to_workflow(error: AcpError) -> Error {
    match error {
        AcpError::Cancelled => Error::Cancelled,
        AcpError::TimedOut { stderr } => {
            if stderr.is_empty() {
                Error::handler("ACP turn timed out")
            } else {
                Error::handler(format!("ACP turn timed out: {stderr}"))
            }
        }
        AcpError::StopReason { stop_reason, text } => {
            Error::handler(format!("ACP prompt stopped with {stop_reason}: {text}"))
        }
        other => Error::handler_with_source("ACP turn failed", &other),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use fabro_agent::{LocalSandbox, Sandbox, shell_quote};
    use fabro_graphviz::graph::{AttrValue, Node};
    use fabro_model::Provider;
    use fabro_types::EventBody;
    use tokio_util::sync::CancellationToken;

    use super::AgentAcpBackend;
    use crate::context::Context;
    use crate::event::{Emitter, StageScope};
    use crate::handler::agent::{CodergenBackend, CodergenResult};

    #[tokio::test]
    async fn acp_backend_run_sends_prompt_and_returns_text() {
        let tempdir = tempfile::tempdir().unwrap();
        init_git(tempdir.path());
        let script_path = tempdir.path().join("fake_acp_agent.py");
        tokio::fs::write(&script_path, fake_agent_script())
            .await
            .unwrap();

        let mut node = Node::new("work");
        node.attrs.insert(
            "provider".to_string(),
            AttrValue::String("openai".to_string()),
        );
        node.attrs.insert(
            "model".to_string(),
            AttrValue::String("fake-acp".to_string()),
        );
        node.attrs
            .insert("backend".to_string(), AttrValue::String("acp".to_string()));
        node.attrs.insert(
            "acp_command".to_string(),
            AttrValue::String(format!(
                "python3 {}",
                shell_quote(&script_path.to_string_lossy())
            )),
        );

        let backend = AgentAcpBackend::new_from_env("fake-acp".to_string(), Provider::OpenAi);
        let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new(tempdir.path().to_path_buf()));
        let result = backend
            .run(
                &node,
                "write hello",
                &Context::new(),
                None,
                &Arc::new(Emitter::default()),
                &sandbox,
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let CodergenResult::Text {
            text,
            files_touched,
            ..
        } = result
        else {
            panic!("expected text result");
        };
        assert_eq!(text, "hello from acp");
        assert_eq!(files_touched, vec!["hello.txt"]);
    }

    #[tokio::test]
    async fn acp_backend_one_shot_combines_system_prompt_and_uses_passed_sandbox() {
        let tempdir = tempfile::tempdir().unwrap();
        let script_path = tempdir.path().join("fake_acp_agent.py");
        let prompt_record_path = tempdir.path().join("prompt.json");
        tokio::fs::write(&script_path, fake_agent_script())
            .await
            .unwrap();

        let mut node = Node::new("prompt");
        node.attrs.insert(
            "provider".to_string(),
            AttrValue::String("openai".to_string()),
        );
        node.attrs
            .insert("backend".to_string(), AttrValue::String("acp".to_string()));
        node.attrs.insert(
            "acp_command".to_string(),
            AttrValue::String(format!(
                "python3 {}",
                shell_quote(&script_path.to_string_lossy())
            )),
        );

        let backend = AgentAcpBackend::new_from_env("fake-acp".to_string(), Provider::OpenAi)
            .with_env(HashMap::from([(
                "ACP_PROMPT_RECORD".to_string(),
                prompt_record_path.to_string_lossy().into_owned(),
            )]));
        let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new(tempdir.path().to_path_buf()));
        let result = backend
            .one_shot(
                &node,
                "User prompt",
                Some("System prompt"),
                &Arc::new(Emitter::default()),
                &StageScope::for_handler(&Context::new(), "prompt"),
                &sandbox,
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert!(matches!(result, CodergenResult::Text { .. }));
        let recorded = tokio::fs::read_to_string(prompt_record_path).await.unwrap();
        assert!(recorded.contains("System:\\nSystem prompt\\n\\nUser:\\nUser prompt"));
        assert_eq!(
            tokio::fs::read_to_string(tempdir.path().join("hello.txt"))
                .await
                .unwrap(),
            "hello from sandbox\n"
        );
    }

    #[tokio::test]
    async fn acp_backend_cancelled_stop_reason_maps_to_cancelled_error() {
        let tempdir = tempfile::tempdir().unwrap();
        let script_path = tempdir.path().join("fake_acp_agent.py");
        tokio::fs::write(&script_path, fake_agent_script())
            .await
            .unwrap();

        let mut node = Node::new("work");
        node.attrs.insert(
            "provider".to_string(),
            AttrValue::String("openai".to_string()),
        );
        node.attrs.insert(
            "acp_command".to_string(),
            AttrValue::String(format!(
                "python3 {}",
                shell_quote(&script_path.to_string_lossy())
            )),
        );

        let backend =
            AgentAcpBackend::new_from_env("fake-acp".to_string(), Provider::OpenAi).with_env(
                HashMap::from([("ACP_STOP_REASON".to_string(), "cancelled".to_string())]),
            );
        let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new(tempdir.path().to_path_buf()));
        let result = backend
            .run(
                &node,
                "cancel",
                &Context::new(),
                None,
                &Arc::new(Emitter::default()),
                &sandbox,
                None,
                CancellationToken::new(),
            )
            .await;
        let Err(err) = result else {
            panic!("expected cancellation error");
        };

        assert!(matches!(err, crate::error::Error::Cancelled));
    }

    #[tokio::test]
    async fn acp_started_event_omits_json_command_env_values() {
        let tempdir = tempfile::tempdir().unwrap();
        let script_path = tempdir.path().join("fake_acp_agent.py");
        tokio::fs::write(&script_path, fake_agent_script())
            .await
            .unwrap();

        let raw_command = serde_json::json!({
            "type": "stdio",
            "name": "fake",
            "command": "python3",
            "args": [script_path.to_string_lossy()],
            "env": [
                {"name": "OPENAI_API_KEY", "value": "secret-key"}
            ],
        })
        .to_string();
        let mut node = Node::new("work");
        node.attrs.insert(
            "provider".to_string(),
            AttrValue::String("openai".to_string()),
        );
        node.attrs
            .insert("backend".to_string(), AttrValue::String("acp".to_string()));
        node.attrs
            .insert("acp_command".to_string(), AttrValue::String(raw_command));

        let backend = AgentAcpBackend::new_from_env("fake-acp".to_string(), Provider::OpenAi);
        let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new(tempdir.path().to_path_buf()));
        let emitter = Arc::new(Emitter::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        emitter.on_event({
            let events = Arc::clone(&events);
            move |event| events.lock().unwrap().push(event.clone())
        });

        backend
            .run(
                &node,
                "write hello",
                &Context::new(),
                None,
                &emitter,
                &sandbox,
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let events = events.lock().unwrap();
        let command = events
            .iter()
            .find_map(|event| match &event.body {
                EventBody::AgentAcpStarted(props) => Some(props.command.as_str()),
                _ => None,
            })
            .expect("ACP started event should be emitted");
        assert!(command.contains("python3"));
        assert!(command.contains("fake_acp_agent.py"));
        assert!(!command.contains("OPENAI_API_KEY"));
        assert!(!command.contains("secret-key"));
    }

    fn fake_agent_script() -> &'static str {
        r#"
import json
import os
import sys

session_id = "sess-1"

def send(message):
    print(json.dumps(message), flush=True)

def respond(message, result):
    send({"jsonrpc": "2.0", "id": message["id"], "result": result})

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        respond(message, {"protocolVersion": 1, "agentCapabilities": {}})
    elif method == "session/new":
        respond(message, {"sessionId": session_id})
    elif method == "session/prompt":
        if os.environ.get("ACP_PROMPT_RECORD"):
            with open(os.environ["ACP_PROMPT_RECORD"], "w", encoding="utf-8") as record:
                record.write(json.dumps(message.get("params", {})))
        with open("hello.txt", "w", encoding="utf-8") as file:
            file.write("hello from sandbox\n")
        for text in ["hello ", "from acp"]:
            send({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": session_id,
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": text}
                    }
                }
            })
        respond(message, {"stopReason": os.environ.get("ACP_STOP_REASON", "end_turn")})
        break
"#
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "unit test initializes an isolated git repository with the system git binary"
    )]
    fn init_git(path: &std::path::Path) {
        let output = std::process::Command::new("git")
            .arg("init")
            .current_dir(path)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
}
