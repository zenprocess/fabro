#![expect(
    clippy::disallowed_methods,
    reason = "integration tests stage MCP config files with sync std::fs"
)]

use std::path::{Path, PathBuf};

use fabro_test::{fabro_json_snapshot, fabro_snapshot, test_context};

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
