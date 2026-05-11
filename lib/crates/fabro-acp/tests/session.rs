use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::StopReason;
use fabro_acp::{AcpError, AcpRunRequest, AcpRunResult, resolve_acp_command, run_acp_turn};
use fabro_model::Provider;
use fabro_sandbox::{LocalSandbox, Sandbox, shell_quote};
use tokio::fs::{read_to_string, write};
use tokio::process::Command;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn session_lifecycle_initializes_sends_prompt_and_aggregates_text() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let script_path = tempdir.path().join("fake_acp_agent.py");
    let record_path = tempdir.path().join("methods.txt");
    write(&script_path, fake_agent_script())
        .await
        .expect("write fake ACP agent");

    let raw_command = format!("python3 {}", shell_quote(&script_path.to_string_lossy()));
    let command =
        resolve_acp_command(Provider::OpenAi, Some(&raw_command)).expect("resolve ACP command");
    let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new(tempdir.path().to_path_buf()));

    let result = run_acp_turn(AcpRunRequest {
        command,
        prompt: "hello".to_string(),
        cwd: tempdir.path().to_string_lossy().into_owned(),
        timeout_ms: Some(5_000),
        env: HashMap::from([(
            "ACP_RECORD".to_string(),
            record_path.to_string_lossy().into_owned(),
        )]),
        sandbox,
        cancel_token: CancellationToken::new(),
        on_activity: None,
    })
    .await
    .expect("run ACP turn");

    assert_eq!(result.text, "hello from acp");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert_eq!(
        read_to_string(record_path)
            .await
            .expect("read method record"),
        "initialize\nsession/new\nsession/prompt\n"
    );
}

#[tokio::test]
async fn permission_request_selects_allow_always() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let permission_path = tempdir.path().join("permission.json");

    let result = run_fake_agent(
        tempdir.path(),
        HashMap::from([
            ("ACP_MODE".to_string(), "permission".to_string()),
            (
                "ACP_PERMISSION".to_string(),
                permission_path.to_string_lossy().into_owned(),
            ),
        ]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect("run ACP turn");

    assert_eq!(result.text, "hello from acp");
    let permission = read_to_string(permission_path)
        .await
        .expect("read permission record");
    assert!(permission.contains(r#""outcome":"selected""#));
    assert!(permission.contains(r#""optionId":"always""#));
}

#[tokio::test]
async fn runs_inside_sandbox_and_uses_requested_cwd() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let cwd_path = tempdir.path().join("session_new.json");

    let result = run_fake_agent(
        tempdir.path(),
        HashMap::from([
            ("ACP_MODE".to_string(), "write_file".to_string()),
            (
                "ACP_SESSION_NEW_PARAMS".to_string(),
                cwd_path.to_string_lossy().into_owned(),
            ),
        ]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect("run ACP turn");

    assert_eq!(result.text, "hello from acp");
    assert_eq!(
        read_to_string(tempdir.path().join("hello.txt"))
            .await
            .expect("read sandbox output file"),
        "hello from sandbox\n"
    );
    assert!(
        read_to_string(cwd_path)
            .await
            .expect("read session/new params")
            .contains(&tempdir.path().to_string_lossy().into_owned())
    );
}

#[tokio::test]
async fn cancellation_sends_session_cancel_and_returns_cancelled() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let cancel_path = tempdir.path().join("cancel.txt");
    let cancel_token = CancellationToken::new();
    let cancel_for_task = cancel_token.clone();

    let task = tokio::spawn(async move {
        run_fake_agent(
            tempdir.path(),
            HashMap::from([
                ("ACP_MODE".to_string(), "cancel".to_string()),
                (
                    "ACP_CANCEL_RECORD".to_string(),
                    cancel_path.to_string_lossy().into_owned(),
                ),
            ]),
            Some(5_000),
            cancel_for_task,
        )
        .await
    });

    sleep(Duration::from_millis(100)).await;
    cancel_token.cancel();
    let err = task
        .await
        .expect("join cancellation task")
        .expect_err("cancelled turn should error");

    assert!(matches!(err, AcpError::Cancelled));
}

#[tokio::test]
async fn pre_session_cancellation_returns_cancelled() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let cancel_token = CancellationToken::new();
    cancel_token.cancel();

    let err = run_fake_agent(
        tempdir.path(),
        HashMap::from([("ACP_MODE".to_string(), "slow_initialize".to_string())]),
        Some(1_000),
        cancel_token,
    )
    .await
    .expect_err("pre-session cancellation should error");

    assert!(matches!(err, AcpError::Cancelled));
}

#[tokio::test]
async fn successful_turn_terminates_lingering_agent_process() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let pid_path = tempdir.path().join("agent.pid");

    let result = run_fake_agent(
        tempdir.path(),
        HashMap::from([
            ("ACP_MODE".to_string(), "linger_after_response".to_string()),
            (
                "ACP_PID_RECORD".to_string(),
                pid_path.to_string_lossy().into_owned(),
            ),
        ]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect("run ACP turn");

    sleep(Duration::from_millis(100)).await;
    let pid = read_to_string(&pid_path).await.expect("read agent pid");
    let still_running = process_is_running(pid.trim()).await;
    if still_running {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.trim())
            .status()
            .await;
    }

    assert_eq!(result.text, "hello from acp");
    assert!(
        !still_running,
        "successful ACP turn should not leave lingering agent process"
    );
}

#[tokio::test]
async fn refusal_stop_reason_returns_text() {
    let tempdir = tempfile::tempdir().expect("create tempdir");

    let result = run_fake_agent(
        tempdir.path(),
        HashMap::from([("ACP_STOP_REASON".to_string(), "refusal".to_string())]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect("run ACP turn");

    assert_eq!(result.text, "hello from acp");
    assert_eq!(result.stop_reason, StopReason::Refusal);
}

#[tokio::test]
async fn max_tokens_stop_reason_returns_partial_text_error() {
    let tempdir = tempfile::tempdir().expect("create tempdir");

    let err = run_fake_agent(
        tempdir.path(),
        HashMap::from([("ACP_STOP_REASON".to_string(), "max_tokens".to_string())]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect_err("max_tokens should return stop reason error");

    let AcpError::StopReason { stop_reason, text } = err else {
        panic!("expected stop reason error");
    };
    assert_eq!(stop_reason, "max_tokens");
    assert_eq!(text, "hello from acp");
}

#[tokio::test]
async fn max_turn_requests_stop_reason_returns_partial_text_error() {
    let tempdir = tempfile::tempdir().expect("create tempdir");

    let err = run_fake_agent(
        tempdir.path(),
        HashMap::from([(
            "ACP_STOP_REASON".to_string(),
            "max_turn_requests".to_string(),
        )]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect_err("max_turn_requests should return stop reason error");

    let AcpError::StopReason { stop_reason, text } = err else {
        panic!("expected stop reason error");
    };
    assert_eq!(stop_reason, "max_turn_requests");
    assert_eq!(text, "hello from acp");
}

#[tokio::test]
async fn timeout_terminates_process_and_returns_timeout() {
    let tempdir = tempfile::tempdir().expect("create tempdir");

    let err = run_fake_agent(
        tempdir.path(),
        HashMap::from([("ACP_MODE".to_string(), "timeout".to_string())]),
        Some(100),
        CancellationToken::new(),
    )
    .await
    .expect_err("timeout should error");

    assert!(matches!(err, AcpError::TimedOut { .. }));
}

#[tokio::test]
async fn malformed_json_returns_protocol_error() {
    let tempdir = tempfile::tempdir().expect("create tempdir");

    let err = run_fake_agent(
        tempdir.path(),
        HashMap::from([("ACP_MODE".to_string(), "malformed".to_string())]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect_err("malformed JSON should error");

    assert!(matches!(err, AcpError::Protocol(_)));
}

#[tokio::test]
async fn early_exit_returns_protocol_error_with_stderr() {
    let tempdir = tempfile::tempdir().expect("create tempdir");

    let err = run_fake_agent(
        tempdir.path(),
        HashMap::from([("ACP_MODE".to_string(), "early_exit".to_string())]),
        Some(5_000),
        CancellationToken::new(),
    )
    .await
    .expect_err("early exit should error");

    let AcpError::Protocol(error) = err else {
        panic!("expected protocol error");
    };
    let message = error.to_string();
    assert!(
        message.contains("exit_code=2"),
        "early exit should include exit code in diagnostic: {message}"
    );
    assert!(
        message.contains("early boom"),
        "early exit should include stderr tail in diagnostic: {message}"
    );
}

async fn run_fake_agent(
    tempdir: &Path,
    env: HashMap<String, String>,
    timeout_ms: Option<u64>,
    cancel_token: CancellationToken,
) -> Result<AcpRunResult, AcpError> {
    let script_path = tempdir.join("fake_acp_agent.py");
    write(&script_path, fake_agent_script())
        .await
        .expect("write fake ACP agent");
    let raw_command = format!("python3 {}", shell_quote(&script_path.to_string_lossy()));
    let command =
        resolve_acp_command(Provider::OpenAi, Some(&raw_command)).expect("resolve ACP command");
    let sandbox: Arc<dyn Sandbox> = Arc::new(LocalSandbox::new(tempdir.to_path_buf()));

    run_acp_turn(AcpRunRequest {
        command,
        prompt: "hello".to_string(),
        cwd: tempdir.to_string_lossy().into_owned(),
        timeout_ms,
        env,
        sandbox,
        cancel_token,
        on_activity: None,
    })
    .await
}

async fn process_is_running(pid: &str) -> bool {
    let Ok(status) = Command::new("kill").arg("-0").arg(pid).status().await else {
        return false;
    };
    if !status.success() {
        return false;
    }

    let Ok(output) = Command::new("ps")
        .args(["-ww", "-o", "stat=", "-p", pid])
        .output()
        .await
    else {
        return true;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .chars()
        .find(|ch| !ch.is_whitespace())
        .is_none_or(|state| !matches!(state, 'Z' | 'z'))
}

fn fake_agent_script() -> &'static str {
    r#"
import json
import os
import signal
import sys
import time

methods = []
session_id = "sess-1"

if os.environ.get("ACP_PID_RECORD"):
    with open(os.environ["ACP_PID_RECORD"], "w", encoding="utf-8") as record:
        record.write(str(os.getpid()))

def handle_sigterm(signum, frame):
    if os.environ.get("ACP_LINGER_TERMINATED"):
        with open(os.environ["ACP_LINGER_TERMINATED"], "w", encoding="utf-8") as record:
            record.write("terminated\n")
    sys.exit(0)

signal.signal(signal.SIGTERM, handle_sigterm)

def send(message):
    print(json.dumps(message), flush=True)

def respond(message, result):
    send({"jsonrpc": "2.0", "id": message["id"], "result": result})

def record_methods():
    if os.environ.get("ACP_RECORD"):
        with open(os.environ["ACP_RECORD"], "w", encoding="utf-8") as record:
            record.write("\n".join(methods) + "\n")

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    methods.append(method)

    if method == "initialize":
        if os.environ.get("ACP_MODE") == "slow_initialize":
            time.sleep(60)
        respond(message, {"protocolVersion": 1, "agentCapabilities": {}})
    elif method == "session/new":
        if os.environ.get("ACP_SESSION_NEW_PARAMS"):
            with open(os.environ["ACP_SESSION_NEW_PARAMS"], "w", encoding="utf-8") as record:
                record.write(json.dumps(message.get("params", {}), separators=(",", ":")))
        respond(message, {"sessionId": session_id})
    elif method == "session/prompt":
        mode = os.environ.get("ACP_MODE", "normal")
        if mode == "timeout":
            time.sleep(60)
        if mode == "malformed":
            print("malformed json", file=sys.stderr, flush=True)
            print("{not-json", flush=True)
            break
        if mode == "early_exit":
            print("early boom", file=sys.stderr, flush=True)
            sys.exit(2)
        if mode == "write_file":
            with open("hello.txt", "w", encoding="utf-8") as file:
                file.write("hello from sandbox\n")
        if mode == "cancel":
            for cancel_line in sys.stdin:
                cancel_message = json.loads(cancel_line)
                if cancel_message.get("method") == "session/cancel":
                    with open(os.environ["ACP_CANCEL_RECORD"], "w", encoding="utf-8") as record:
                        record.write("session/cancel\n")
                    respond(message, {"stopReason": "cancelled"})
                    sys.exit(0)
        if mode == "permission":
            send({
                "jsonrpc": "2.0",
                "id": "permission-1",
                "method": "session/request_permission",
                "params": {
                    "sessionId": session_id,
                    "toolCall": {"toolCallId": "tool-1"},
                    "options": [
                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                        {"optionId": "once", "name": "Allow once", "kind": "allow_once"},
                        {"optionId": "always", "name": "Allow always", "kind": "allow_always"}
                    ]
                }
            })
            permission_response = json.loads(sys.stdin.readline())
            with open(os.environ["ACP_PERMISSION"], "w", encoding="utf-8") as permission:
                permission.write(json.dumps(permission_response.get("result", {}), separators=(",", ":")))
        send({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "hello "}
                }
            }
        })
        send({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "from acp"}
                }
            }
        })
        record_methods()
        respond(message, {"stopReason": os.environ.get("ACP_STOP_REASON", "end_turn")})
        if mode == "linger_after_response":
            while True:
                time.sleep(1)
        break
    else:
        send({
            "jsonrpc": "2.0",
            "id": message.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        })
"#
}
