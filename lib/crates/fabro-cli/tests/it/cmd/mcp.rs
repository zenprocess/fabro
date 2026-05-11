#![expect(
    clippy::disallowed_methods,
    reason = "integration tests stage MCP config files with sync std::fs"
)]
#![expect(
    clippy::disallowed_types,
    reason = "raw stdio regression test intentionally uses blocking std pipes outside Tokio"
)]

use std::collections::HashMap;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use fabro_mcp::client::McpClient;
use fabro_mcp::config::{McpServerSettings, McpTransport};
use fabro_test::{fabro_json_snapshot, fabro_snapshot, test_context};

use crate::support::{RealAuthHarness, TEST_DEV_TOKEN, seed_dev_token_auth};

#[test]
fn help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"
    success: true
    exit_code: 0
    ----- stdout -----
    Model Context Protocol server

    Usage: fabro mcp [OPTIONS] <COMMAND>

    Commands:
      start   Start the Fabro MCP server over stdio
      config  Print MCP client configuration JSON
      init    Configure an MCP client to launch Fabro
      help    Print this message or the help of the given subcommand(s)

    Options:
          --json              Output as JSON [env: FABRO_JSON=]
          --debug             Enable DEBUG-level logging (default is INFO) [env: FABRO_DEBUG=]
          --no-upgrade-check  Disable automatic upgrade check [env: FABRO_NO_UPGRADE_CHECK=true]
          --quiet             Suppress non-essential output [env: FABRO_QUIET=]
          --verbose           Enable verbose output [env: FABRO_VERBOSE=]
      -h, --help              Print help
    ----- stderr -----
    ");
}

#[test]
fn start_help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "start", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"
    success: true
    exit_code: 0
    ----- stdout -----
    Start the Fabro MCP server over stdio

    Usage: fabro mcp start [OPTIONS]

    Options:
          --json                       Output as JSON [env: FABRO_JSON=]
          --storage-dir <STORAGE_DIR>  Local storage directory (default: ~/.fabro/storage) [env: FABRO_STORAGE_DIR=]
          --debug                      Enable DEBUG-level logging (default is INFO) [env: FABRO_DEBUG=]
          --server <SERVER>            Fabro server target: http(s) URL or absolute Unix socket path [env: FABRO_SERVER=]
          --no-upgrade-check           Disable automatic upgrade check [env: FABRO_NO_UPGRADE_CHECK=true]
          --quiet                      Suppress non-essential output [env: FABRO_QUIET=]
          --verbose                    Enable verbose output [env: FABRO_VERBOSE=]
      -h, --help                       Print help
    ----- stderr -----
    ");
}

#[test]
fn config_help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "config", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"
    success: true
    exit_code: 0
    ----- stdout -----
    Print MCP client configuration JSON

    Usage: fabro mcp config [OPTIONS]

    Options:
          --json                       Output as JSON [env: FABRO_JSON=]
          --storage-dir <STORAGE_DIR>  Local storage directory (default: ~/.fabro/storage) [env: FABRO_STORAGE_DIR=]
          --debug                      Enable DEBUG-level logging (default is INFO) [env: FABRO_DEBUG=]
          --server <SERVER>            Fabro server target: http(s) URL or absolute Unix socket path [env: FABRO_SERVER=]
          --no-upgrade-check           Disable automatic upgrade check [env: FABRO_NO_UPGRADE_CHECK=true]
          --quiet                      Suppress non-essential output [env: FABRO_QUIET=]
          --verbose                    Enable verbose output [env: FABRO_VERBOSE=]
      -h, --help                       Print help
    ----- stderr -----
    ");
}

#[test]
fn init_help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "init", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"
    success: true
    exit_code: 0
    ----- stdout -----
    Configure an MCP client to launch Fabro

    Usage: fabro mcp init [OPTIONS] <AGENT>

    Arguments:
      <AGENT>  [possible values: claude, cursor, windsurf]

    Options:
          --json                       Output as JSON [env: FABRO_JSON=]
          --storage-dir <STORAGE_DIR>  Local storage directory (default: ~/.fabro/storage) [env: FABRO_STORAGE_DIR=]
          --debug                      Enable DEBUG-level logging (default is INFO) [env: FABRO_DEBUG=]
          --server <SERVER>            Fabro server target: http(s) URL or absolute Unix socket path [env: FABRO_SERVER=]
          --no-upgrade-check           Disable automatic upgrade check [env: FABRO_NO_UPGRADE_CHECK=true]
          --quiet                      Suppress non-essential output [env: FABRO_QUIET=]
          --verbose                    Enable verbose output [env: FABRO_VERBOSE=]
      -h, --help                       Print help
    ----- stderr -----
    ");
}

#[test]
fn config_prints_generic_mcp_json() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "config"]);
    fabro_snapshot!(context.filters(), cmd, @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    {
      "mcpServers": {
        "fabro": {
          "command": "fabro",
          "args": [
            "mcp",
            "start"
          ]
        }
      }
    }
    ----- stderr -----
    "#);
}

#[test]
fn config_preserves_connection_flags() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args([
        "mcp",
        "config",
        "--server",
        "https://example.test/api/v1",
        "--storage-dir",
        "/tmp/fabro-mcp-storage",
    ]);
    fabro_snapshot!(context.filters(), cmd, @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    {
      "mcpServers": {
        "fabro": {
          "command": "fabro",
          "args": [
            "mcp",
            "start",
            "--server",
            "https://example.test/api/v1",
            "--storage-dir",
            "/tmp/fabro-mcp-storage"
          ]
        }
      }
    }
    ----- stderr -----
    "#);
}

#[test]
fn init_cursor_writes_idempotent_config() {
    let context = test_context!();
    context
        .command()
        .args(["mcp", "init", "cursor"])
        .assert()
        .success();
    context
        .command()
        .args(["mcp", "init", "cursor"])
        .assert()
        .success();

    let config_path = context.home_dir.join(".cursor").join("mcp.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    fabro_json_snapshot!(context, config, @r#"
    {
      "mcpServers": {
        "fabro": {
          "command": "fabro",
          "args": [
            "mcp",
            "start"
          ]
        }
      }
    }
    "#);
}

#[test]
fn init_claude_writes_platform_config() {
    let context = test_context!();
    context
        .command()
        .args(["mcp", "init", "claude"])
        .assert()
        .success();

    let config_path = expected_claude_config_path(&context.home_dir);
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    fabro_json_snapshot!(context, config, @r#"
    {
      "mcpServers": {
        "fabro": {
          "command": "fabro",
          "args": [
            "mcp",
            "start"
          ]
        }
      }
    }
    "#);
}

#[test]
fn init_windsurf_writes_config() {
    let context = test_context!();
    context
        .command()
        .args(["mcp", "init", "windsurf"])
        .assert()
        .success();

    let config_path = context
        .home_dir
        .join(".codeium")
        .join("windsurf")
        .join("mcp_config.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    fabro_json_snapshot!(context, config, @r#"
    {
      "mcpServers": {
        "fabro": {
          "command": "fabro",
          "args": [
            "mcp",
            "start"
          ]
        }
      }
    }
    "#);
}

#[test]
fn init_preserves_existing_servers() {
    let context = test_context!();
    let config_path = context.home_dir.join(".cursor").join("mcp.json");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        r#"{"mcpServers":{"other":{"command":"other","args":["serve"]}},"theme":"dark"}"#,
    )
    .unwrap();

    context
        .command()
        .args([
            "mcp",
            "init",
            "cursor",
            "--server",
            "https://example.test/api/v1",
        ])
        .assert()
        .success();

    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    fabro_json_snapshot!(context, config, @r#"
    {
      "mcpServers": {
        "other": {
          "command": "other",
          "args": [
            "serve"
          ]
        },
        "fabro": {
          "command": "fabro",
          "args": [
            "mcp",
            "start",
            "--server",
            "https://example.test/api/v1"
          ]
        }
      },
      "theme": "dark"
    }
    "#);
}

#[test]
fn init_invalid_json_fails_without_overwrite() {
    let context = test_context!();
    let config_path = context.home_dir.join(".cursor").join("mcp.json");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(&config_path, "{not json").unwrap();

    let mut cmd = context.command();
    cmd.args(["mcp", "init", "cursor"]);
    fabro_snapshot!(context.filters(), cmd, @"
    success: false
    exit_code: 1
    ----- stdout -----
    ----- stderr -----
      × failed to parse MCP config [HOME_DIR]/.cursor/mcp.json
      ╰─▶ key must be a string at line 1 column 2
    ");
    assert_eq!(std::fs::read_to_string(config_path).unwrap(), "{not json");
}

#[tokio::test(flavor = "multi_thread")]
async fn stdio_server_initializes_and_lists_run_tools() {
    let context = test_context!();
    let client = spawn_mcp_client(&context, &[]).await;

    let tools = client.list_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(names, vec![
        "fabro_run_create",
        "fabro_run_events",
        "fabro_run_gather",
        "fabro_run_interact",
        "fabro_run_search",
    ]);
    for (_, _, schema) in tools {
        assert!(
            schema.is_object(),
            "tool should have input schema: {schema}"
        );
    }
}

#[test]
fn stdio_start_writes_only_json_rpc_to_stdout() {
    let context = test_context!();
    let fixture = mcp_stdio_fixture(&context, &[]);
    let mut cmd = std::process::Command::new(&fixture.command[0]);
    cmd.args(&fixture.command[1..])
        .env_clear()
        .envs(&fixture.env)
        .current_dir(&fixture.current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"fabro-test","version":"0.0.0"}}}}}}"#
    )
    .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(result.map(|_| line));
    });

    let line = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("initialize response should arrive")
        .expect("stdout should be readable");
    let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(value["jsonrpc"], "2.0");

    let _ = child.kill();
    let _ = child.wait();
}

#[tokio::test(flavor = "multi_thread")]
async fn stdio_startup_and_list_tools_is_fast() {
    let context = test_context!();
    let start = std::time::Instant::now();
    let client = spawn_mcp_client(&context, &[]).await;
    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 5);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_create_and_search_manage_real_runs_with_cli_auth() {
    let context = test_context!();
    let harness =
        RealAuthHarness::start_with_dev_token(fabro_test::GitHubAppState::default()).await;
    let target_url = harness.api_target();
    let target: fabro_client::ServerTarget = target_url.parse().unwrap();
    seed_dev_token_auth(&context.home_dir, &target, TEST_DEV_TOKEN);
    let workflow = context.install_fixture("simple.fabro");

    let client = spawn_mcp_client(&context, &["--server", &target_url]).await;

    let create = call_tool_json(
        &client,
        "fabro_run_create",
        serde_json::json!({
            "runs": [{
                "workflow": workflow,
                "dry_run": true,
                "auto_approve": true,
                "labels": { "source": "mcp-test" }
            }]
        }),
    )
    .await;
    let run_id = create["runs"][0]["run_id"].as_str().unwrap().to_string();
    assert_eq!(create["runs"][0]["started"], true);

    let search = call_tool_json(
        &client,
        "fabro_run_search",
        serde_json::json!({
            "run_ids": [run_id],
            "labels": { "source": "mcp-test" },
            "first": 10
        }),
    )
    .await;
    fabro_json_snapshot!(context, normalize_run_search(search), @r#"
    {
      "runs": [
        {
          "run_id": "[RUN_ID]",
          "workflow_name": "Simple",
          "workflow_slug": "simple",
          "status": "queued",
          "archived": false,
          "created_at": "[TIMESTAMP]",
          "started_at": null,
          "completed_at": null,
          "labels": {
            "source": "mcp-test"
          },
          "source_directory": "[SOURCE_DIRECTORY]",
          "repo_origin_url": null,
          "goal": "Run the Fabro workflow."
        }
      ],
      "next_cursor": null
    }
    "#);

    harness.shutdown().await;
}

fn expected_claude_config_path(home_dir: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home_dir
            .join("Library")
            .join("Application Support")
            .join("Claude")
            .join("claude_desktop_config.json")
    }
    #[cfg(target_os = "linux")]
    {
        home_dir
            .join(".config")
            .join("Claude")
            .join("claude_desktop_config.json")
    }
    #[cfg(target_os = "windows")]
    {
        home_dir
            .join("AppData")
            .join("Roaming")
            .join("Claude")
            .join("claude_desktop_config.json")
    }
}

struct McpStdioFixture {
    command:     Vec<String>,
    env:         HashMap<String, String>,
    current_dir: PathBuf,
}

fn mcp_stdio_fixture(context: &fabro_test::TestContext, extra_args: &[&str]) -> McpStdioFixture {
    let mut command = vec![
        env!("CARGO_BIN_EXE_fabro").to_string(),
        "mcp".to_string(),
        "start".to_string(),
    ];
    command.extend(extra_args.iter().map(|arg| (*arg).to_string()));

    let mut env = fabro_test::isolated_env(&context.home_dir);
    env.insert(
        "FABRO_HOME".to_string(),
        context.home_dir.join(".fabro").display().to_string(),
    );

    McpStdioFixture {
        command,
        env,
        current_dir: context.temp_dir.clone(),
    }
}

async fn spawn_mcp_client(context: &fabro_test::TestContext, extra_args: &[&str]) -> McpClient {
    let fixture = mcp_stdio_fixture(context, extra_args);
    let config = McpServerSettings {
        name:                 "fabro-under-test".to_string(),
        transport:            McpTransport::Stdio {
            command: fixture.command,
            env:     fixture.env,
        },
        startup_timeout_secs: 10,
        tool_timeout_secs:    30,
    };
    let client = McpClient::new(&config).expect("MCP client should build");
    client
        .initialize(config.startup_timeout())
        .await
        .expect("MCP server should initialize");
    client
}

async fn call_tool_json(
    client: &McpClient,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let result = client
        .call_tool(name, arguments, std::time::Duration::from_secs(30))
        .await
        .expect("tool call should complete");
    assert_ne!(
        result.is_error,
        Some(true),
        "tool returned error: {result:?}"
    );
    let text = result
        .content
        .first()
        .and_then(|content| serde_json::to_value(content).ok())
        .and_then(|content| content["text"].as_str().map(ToOwned::to_owned))
        .expect("tool result should include text fallback");
    assert!(!text.starts_with('{') && !text.starts_with('['));
    result
        .structured_content
        .expect("tool result should include structured content")
}

fn normalize_run_search(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(runs) = value["runs"].as_array_mut() {
        for run in runs {
            run["run_id"] = serde_json::json!("[RUN_ID]");
            run["created_at"] = serde_json::json!("[TIMESTAMP]");
            if run["started_at"].is_string() {
                run["started_at"] = serde_json::json!("[TIMESTAMP]");
            }
            if run["completed_at"].is_string() {
                run["completed_at"] = serde_json::json!("[TIMESTAMP]");
            }
            if run["source_directory"].is_string() {
                run["source_directory"] = serde_json::json!("[SOURCE_DIRECTORY]");
            }
        }
    }
    value
}
