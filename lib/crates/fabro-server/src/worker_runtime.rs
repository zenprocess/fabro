use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, InspectContainerOptions, KillContainerOptions, LogOutput,
    LogsOptions, RemoveContainerOptions, StartContainerOptions, WaitContainerOptions,
};
use bollard::errors::Error as DockerError;
use bollard::models::HostConfig;
use fabro_static::EnvVars;
use fabro_types::RunId;
use fabro_types::settings::server::LogDestination;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::spawn_env::apply_worker_env;

const DOCKER_WORKER_STORAGE_DIR: &str = "/tmp/fabro-worker/storage";
const DOCKER_WORKER_RUN_DIR: &str = "/tmp/fabro-worker/run";
const DOCKER_SOCKET_CONTAINER_PATH: &str = "/var/run/docker.sock";

const DOCKER_WORKER_ENV_ALLOWLIST: &[&str] = &[
    EnvVars::RUST_LOG,
    EnvVars::RUST_BACKTRACE,
    EnvVars::FABRO_LOG,
    EnvVars::TERM,
    EnvVars::NO_COLOR,
    EnvVars::CLICOLOR,
    EnvVars::CLICOLOR_FORCE,
];

#[async_trait]
pub(crate) trait WorkerRuntime: Send + Sync {
    #[cfg(test)]
    fn runtime_kind(&self) -> WorkerRuntimeKind {
        WorkerRuntimeKind::Custom
    }

    async fn start(&self, spec: WorkerLaunchSpec) -> Result<StartedWorker>;
    async fn request_stop(&self, worker_ref: &WorkerRef);
    async fn force_stop(&self, worker_ref: &WorkerRef);
    async fn is_alive(&self, worker_ref: &WorkerRef) -> bool;
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerRuntimeKind {
    Local,
    Docker,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkerRef {
    /// A worker running as a local subprocess. `pre_exec_setpgid` ensures the
    /// child is the leader of its own process group with `pgid == pid`, so a
    /// single PID identifies both the process and its group.
    Local {
        pid: u32,
    },
    Docker {
        container_id: String,
    },
}

pub(crate) enum WorkerLaunchSpec {
    Local(LocalWorkerLaunchSpec),
    Docker(DockerWorkerLaunchSpec),
}

pub(crate) struct WorkerLaunchCommon {
    pub(crate) run_id:          RunId,
    pub(crate) mode:            &'static str,
    pub(crate) worker_token:    String,
    pub(crate) log_destination: LogDestination,
    pub(crate) fabro_log:       Option<String>,
}

pub(crate) struct LocalWorkerLaunchSpec {
    pub(crate) common:                 WorkerLaunchCommon,
    pub(crate) executable:             PathBuf,
    pub(crate) server_target:          String,
    pub(crate) storage_dir:            PathBuf,
    pub(crate) run_dir:                PathBuf,
    pub(crate) active_config_path:     PathBuf,
    pub(crate) github_app_private_key: Option<String>,
}

pub(crate) struct DockerWorkerLaunchSpec {
    pub(crate) common:         WorkerLaunchCommon,
    pub(crate) image:          String,
    pub(crate) server_url:     String,
    pub(crate) network:        Option<String>,
    pub(crate) docker_socket:  Option<PathBuf>,
    pub(crate) remove_on_exit: bool,
}

pub(crate) struct StartedWorker {
    pub(crate) worker_ref: WorkerRef,
    pub(crate) output:     BoxStream<'static, Result<WorkerOutputLine>>,
    pub(crate) wait:       BoxFuture<'static, Result<WorkerExit>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerOutputStreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerOutputLine {
    pub(crate) stream: WorkerOutputStreamKind,
    pub(crate) line:   String,
}

#[derive(Debug)]
pub(crate) struct WorkerExit {
    pub(crate) success: bool,
    pub(crate) detail:  String,
}

#[derive(Default)]
pub(crate) struct LocalWorkerRuntime;

impl LocalWorkerRuntime {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn command_for_spec(spec: &LocalWorkerLaunchSpec) -> Command {
        let common = &spec.common;
        let worker_stdout = match common.log_destination {
            LogDestination::Stdout => Stdio::inherit(),
            LogDestination::File => Stdio::null(),
        };
        let log_destination_env: &'static str = common.log_destination.into();

        let mut cmd = Command::new(&spec.executable);
        cmd.arg("__run-worker")
            .arg("--server")
            .arg(&spec.server_target)
            .arg("--storage-dir")
            .arg(&spec.storage_dir)
            .arg("--run-dir")
            .arg(&spec.run_dir)
            .arg("--run-id")
            .arg(common.run_id.to_string())
            .arg("--mode")
            .arg(common.mode)
            .stdin(Stdio::null())
            .stdout(worker_stdout)
            .stderr(Stdio::piped());

        apply_worker_env(&mut cmd);
        if let Some(level) = common.fabro_log.as_deref() {
            cmd.env(EnvVars::FABRO_LOG, level);
        }
        cmd.env(EnvVars::FABRO_LOG_DESTINATION, log_destination_env);
        cmd.env(EnvVars::FABRO_CONFIG, &spec.active_config_path);
        cmd.env_remove(EnvVars::FABRO_WORKER_TOKEN);
        cmd.env(EnvVars::FABRO_WORKER_TOKEN, &common.worker_token);
        if let Some(pem) = spec.github_app_private_key.as_deref() {
            cmd.env(EnvVars::GITHUB_APP_PRIVATE_KEY, pem);
        }

        #[cfg(unix)]
        fabro_proc::pre_exec_setpgid(cmd.as_std_mut());

        cmd
    }
}

#[async_trait]
impl WorkerRuntime for LocalWorkerRuntime {
    #[cfg(test)]
    fn runtime_kind(&self) -> WorkerRuntimeKind {
        WorkerRuntimeKind::Local
    }

    async fn start(&self, spec: WorkerLaunchSpec) -> Result<StartedWorker> {
        let WorkerLaunchSpec::Local(spec) = spec else {
            anyhow::bail!("local worker runtime received Docker launch spec");
        };
        let mut child = Self::command_for_spec(&spec)
            .spawn()
            .context("spawning run worker process")?;

        let Some(pid) = child.id() else {
            let _ = child.start_kill();
            anyhow::bail!("worker process did not report a PID");
        };
        let Some(stderr) = child.stderr.take() else {
            let _ = child.start_kill();
            anyhow::bail!("worker child stderr should be piped");
        };
        let output = local_worker_output_stream(stderr);
        let wait: BoxFuture<'static, Result<WorkerExit>> = Box::pin(async move {
            let status = child.wait().await.context("worker wait failed")?;
            Ok(WorkerExit {
                success: status.success(),
                detail:  status.to_string(),
            })
        });

        Ok(StartedWorker {
            worker_ref: WorkerRef::Local { pid },
            output,
            wait,
        })
    }

    async fn request_stop(&self, worker_ref: &WorkerRef) {
        if let WorkerRef::Local { pid } = worker_ref {
            #[cfg(unix)]
            fabro_proc::sigterm_process_group(*pid);
            #[cfg(not(unix))]
            let _ = pid;
        }
    }

    async fn force_stop(&self, worker_ref: &WorkerRef) {
        if let WorkerRef::Local { pid } = worker_ref {
            #[cfg(unix)]
            fabro_proc::sigkill_process_group(*pid);
            #[cfg(not(unix))]
            let _ = pid;
        }
    }

    async fn is_alive(&self, worker_ref: &WorkerRef) -> bool {
        if let WorkerRef::Local { pid } = worker_ref {
            #[cfg(unix)]
            {
                return fabro_proc::process_group_alive(*pid);
            }
            #[cfg(not(unix))]
            {
                return fabro_proc::process_running(*pid);
            }
        }
        false
    }
}

pub(crate) struct DockerWorkerRuntime {
    docker:         Docker,
    remove_on_exit: Arc<Mutex<HashMap<String, bool>>>,
}

impl DockerWorkerRuntime {
    pub(crate) fn new() -> Result<Self> {
        let docker =
            Docker::connect_with_local_defaults().context("failed to connect to Docker daemon")?;
        Ok(Self::from_docker(docker))
    }

    fn from_docker(docker: Docker) -> Self {
        Self {
            docker,
            remove_on_exit: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl WorkerRuntime for DockerWorkerRuntime {
    #[cfg(test)]
    fn runtime_kind(&self) -> WorkerRuntimeKind {
        WorkerRuntimeKind::Docker
    }

    async fn start(&self, spec: WorkerLaunchSpec) -> Result<StartedWorker> {
        let WorkerLaunchSpec::Docker(spec) = spec else {
            anyhow::bail!("Docker worker runtime received local launch spec");
        };

        let (container_id, _container_name) =
            create_docker_worker_container(&self.docker, &spec).await?;
        self.remove_on_exit
            .lock()
            .await
            .insert(container_id.clone(), spec.remove_on_exit);
        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .with_context(|| format!("failed to start Docker worker container {container_id}"))?;

        let output = docker_worker_output_stream(self.docker.clone(), container_id.clone());
        let docker = self.docker.clone();
        let remove_on_exit = Arc::clone(&self.remove_on_exit);
        let wait_container_id = container_id.clone();
        let wait: BoxFuture<'static, Result<WorkerExit>> = Box::pin(async move {
            let policy = remove_on_exit
                .lock()
                .await
                .get(&wait_container_id)
                .copied()
                .unwrap_or(spec.remove_on_exit);
            let exit = wait_for_docker_worker(&docker, &wait_container_id).await;
            remove_on_exit.lock().await.remove(&wait_container_id);
            if policy {
                remove_docker_worker_container(&docker, &wait_container_id).await;
            }
            exit
        });

        Ok(StartedWorker {
            worker_ref: WorkerRef::Docker { container_id },
            output,
            wait,
        })
    }

    async fn request_stop(&self, worker_ref: &WorkerRef) {
        let WorkerRef::Docker { container_id } = worker_ref else {
            return;
        };
        kill_docker_worker(&self.docker, container_id, "SIGTERM").await;
    }

    async fn force_stop(&self, worker_ref: &WorkerRef) {
        let WorkerRef::Docker { container_id } = worker_ref else {
            return;
        };
        kill_docker_worker(&self.docker, container_id, "SIGKILL").await;
        let remove = self
            .remove_on_exit
            .lock()
            .await
            .get(container_id)
            .copied()
            .unwrap_or(false);
        if remove {
            remove_docker_worker_container(&self.docker, container_id).await;
        }
    }

    async fn is_alive(&self, worker_ref: &WorkerRef) -> bool {
        let WorkerRef::Docker { container_id } = worker_ref else {
            return false;
        };
        let Ok(details) = self
            .docker
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
        else {
            return false;
        };
        let Some(state) = details.state else {
            return false;
        };
        state.running.unwrap_or(false) || state.restarting.unwrap_or(false)
    }
}

fn local_worker_output_stream<R>(stderr: R) -> BoxStream<'static, Result<WorkerOutputLine>>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if tx
                        .send(Ok(WorkerOutputLine {
                            stream: WorkerOutputStreamKind::Stderr,
                            line,
                        }))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    let _ = tx.send(Err(
                        anyhow::Error::new(err).context("failed to read worker stderr")
                    ));
                    break;
                }
            }
        }
    });
    UnboundedReceiverStream::new(rx).boxed()
}

async fn create_docker_worker_container(
    docker: &Docker,
    spec: &DockerWorkerLaunchSpec,
) -> Result<(String, String)> {
    let mut last_error = None;
    for attempt in 0..2 {
        let name = docker_worker_container_name(&spec.common.run_id);
        let options = Some(CreateContainerOptions {
            name:     name.clone(),
            platform: None,
        });
        match docker
            .create_container(options, docker_worker_container_config(spec))
            .await
        {
            Ok(container) => return Ok((container.id, name)),
            Err(err) if attempt == 0 && docker_status_code(&err) == Some(409) => {
                last_error = Some(err);
            }
            Err(err) => {
                return Err(
                    anyhow::Error::new(err).context("failed to create Docker worker container")
                );
            }
        }
    }

    let err = last_error.expect("name-conflict retry should retain the last Docker error");
    Err(anyhow::Error::new(err).context("failed to create Docker worker container"))
}

fn docker_worker_container_config(spec: &DockerWorkerLaunchSpec) -> Config<String> {
    let host_config = HostConfig {
        binds: docker_socket_bind(spec),
        network_mode: spec.network.clone(),
        ..Default::default()
    };
    Config {
        image: Some(spec.image.clone()),
        cmd: Some(docker_worker_command(spec)),
        env: Some(docker_worker_env(&spec.common)),
        labels: Some(docker_worker_labels(&spec.common.run_id)),
        host_config: Some(host_config),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        open_stdin: Some(false),
        tty: Some(false),
        ..Default::default()
    }
}

fn docker_worker_command(spec: &DockerWorkerLaunchSpec) -> Vec<String> {
    vec![
        "fabro".to_string(),
        "__run-worker".to_string(),
        "--bootstrap".to_string(),
        "api".to_string(),
        "--server".to_string(),
        spec.server_url.clone(),
        "--storage-dir".to_string(),
        DOCKER_WORKER_STORAGE_DIR.to_string(),
        "--run-dir".to_string(),
        DOCKER_WORKER_RUN_DIR.to_string(),
        "--run-id".to_string(),
        spec.common.run_id.to_string(),
        "--mode".to_string(),
        spec.common.mode.to_string(),
    ]
}

fn docker_worker_labels(run_id: &RunId) -> HashMap<String, String> {
    HashMap::from([
        ("sh.fabro.managed".to_string(), "true".to_string()),
        ("sh.fabro.role".to_string(), "worker".to_string()),
        ("sh.fabro.run_id".to_string(), run_id.to_string()),
    ])
}

fn docker_socket_bind(spec: &DockerWorkerLaunchSpec) -> Option<Vec<String>> {
    spec.docker_socket.as_ref().map(|socket| {
        vec![format!(
            "{}:{DOCKER_SOCKET_CONTAINER_PATH}",
            socket.display()
        )]
    })
}

fn docker_worker_env(common: &WorkerLaunchCommon) -> Vec<String> {
    docker_worker_env_with_lookup(common, &process_env_var)
}

fn docker_worker_env_with_lookup(
    common: &WorkerLaunchCommon,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut env = Vec::new();
    for key in DOCKER_WORKER_ENV_ALLOWLIST {
        if let Some(value) = lookup(key) {
            upsert_env(&mut env, key, &value);
        }
    }
    if let Some(level) = common.fabro_log.as_deref() {
        upsert_env(&mut env, EnvVars::FABRO_LOG, level);
    }
    let log_destination: &'static str = common.log_destination.into();
    upsert_env(&mut env, EnvVars::FABRO_LOG_DESTINATION, log_destination);
    upsert_env(&mut env, EnvVars::FABRO_WORKER_TOKEN, &common.worker_token);
    env
}

fn upsert_env(env: &mut Vec<String>, key: &str, value: &str) {
    let prefix = format!("{key}=");
    if let Some(entry) = env.iter_mut().find(|entry| entry.starts_with(&prefix)) {
        *entry = format!("{key}={value}");
    } else {
        env.push(format!("{key}={value}"));
    }
}

fn docker_worker_container_name(run_id: &RunId) -> String {
    let suffix = ulid::Ulid::new().to_string().to_ascii_lowercase();
    format!("fabro-worker-{run_id}-{suffix}")
}

fn docker_worker_output_stream(
    docker: Docker,
    container_id: String,
) -> BoxStream<'static, Result<WorkerOutputLine>> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut logs = docker.logs::<String>(
            &container_id,
            Some(LogsOptions {
                follow: true,
                stdout: true,
                stderr: true,
                tail: "all".to_string(),
                ..Default::default()
            }),
        );
        while let Some(item) = logs.next().await {
            match item {
                Ok(output) => {
                    for line in worker_lines_from_log_output(output) {
                        if tx.send(Ok(line)).is_err() {
                            return;
                        }
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err(
                        anyhow::Error::new(err).context("failed to read Docker worker logs")
                    ));
                    return;
                }
            }
        }
    });
    UnboundedReceiverStream::new(rx).boxed()
}

fn worker_lines_from_log_output(output: LogOutput) -> Vec<WorkerOutputLine> {
    let (stream, message) = match output {
        LogOutput::StdErr { message } => (WorkerOutputStreamKind::Stderr, message),
        LogOutput::StdOut { message } | LogOutput::Console { message } => {
            (WorkerOutputStreamKind::Stdout, message)
        }
        LogOutput::StdIn { .. } => return Vec::new(),
    };
    String::from_utf8_lossy(&message)
        .lines()
        .map(|line| WorkerOutputLine {
            stream,
            line: line.to_string(),
        })
        .collect()
}

async fn wait_for_docker_worker(docker: &Docker, container_id: &str) -> Result<WorkerExit> {
    let mut stream = docker.wait_container::<String>(
        container_id,
        Some(WaitContainerOptions {
            condition: "not-running".to_string(),
        }),
    );
    let Some(result) = stream.next().await else {
        anyhow::bail!("Docker worker wait stream ended before container exit");
    };
    let status_code = match result {
        Ok(exit) => exit.status_code,
        Err(DockerError::DockerContainerWaitError { code, .. }) => code,
        Err(err) => {
            return Err(
                anyhow::Error::new(err).context("failed while waiting for Docker worker container")
            );
        }
    };
    Ok(WorkerExit {
        success: status_code == 0,
        detail:  format!("Docker container exited with status {status_code}"),
    })
}

async fn kill_docker_worker(docker: &Docker, container_id: &str, signal: &str) {
    if let Err(err) = docker
        .kill_container(
            container_id,
            Some(KillContainerOptions {
                signal: signal.to_string(),
            }),
        )
        .await
    {
        tracing::debug!(
            container_id,
            signal,
            error = %err,
            "Failed to signal Docker worker container"
        );
    }
}

async fn remove_docker_worker_container(docker: &Docker, container_id: &str) {
    if let Err(err) = docker
        .remove_container(
            container_id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
    {
        if docker_status_code(&err) != Some(404) {
            tracing::warn!(
                container_id,
                error = %err,
                "Failed to remove Docker worker container"
            );
        }
    }
}

fn docker_status_code(err: &DockerError) -> Option<u16> {
    match err {
        DockerError::DockerResponseServerError { status_code, .. } => Some(*status_code),
        _ => None,
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "Docker worker env allowlist intentionally copies a narrow process-env subset."
)]
fn process_env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_common() -> WorkerLaunchCommon {
        WorkerLaunchCommon {
            run_id:          RunId::new(),
            mode:            "start",
            worker_token:    "worker-token".to_string(),
            log_destination: LogDestination::File,
            fabro_log:       Some("debug".to_string()),
        }
    }

    fn test_docker_spec() -> DockerWorkerLaunchSpec {
        DockerWorkerLaunchSpec {
            common:         test_common(),
            image:          "ghcr.io/fabro-sh/fabro-worker:test".to_string(),
            server_url:     "http://fabro-server:3333".to_string(),
            network:        Some("fabro-net".to_string()),
            docker_socket:  None,
            remove_on_exit: true,
        }
    }

    #[test]
    fn worker_runtime_docker_container_config_uses_worker_contract() {
        let spec = test_docker_spec();

        let config = docker_worker_container_config(&spec);

        assert_eq!(config.image.as_deref(), Some(spec.image.as_str()));
        assert_eq!(config.cmd, Some(docker_worker_command(&spec)));
        let labels = config.labels.unwrap();
        assert_eq!(
            labels.get("sh.fabro.managed").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            labels.get("sh.fabro.role").map(String::as_str),
            Some("worker")
        );
        let run_id_label = spec.common.run_id.to_string();
        assert_eq!(
            labels.get("sh.fabro.run_id").map(String::as_str),
            Some(run_id_label.as_str())
        );
        let host_config = config.host_config.unwrap();
        assert_eq!(host_config.network_mode.as_deref(), Some("fabro-net"));
        assert!(host_config.binds.is_none());
        assert_eq!(config.open_stdin, Some(false));
        assert_eq!(config.attach_stdout, Some(true));
        assert_eq!(config.attach_stderr, Some(true));
    }

    #[test]
    fn worker_runtime_docker_env_is_fail_closed() {
        let common = test_common();
        let env = docker_worker_env_with_lookup(&common, &|key| {
            HashMap::from([
                (EnvVars::RUST_LOG, "trace"),
                (EnvVars::RUST_BACKTRACE, "1"),
                (EnvVars::FABRO_LOG, "info"),
                (EnvVars::TERM, "xterm-256color"),
                (EnvVars::NO_COLOR, "1"),
                (EnvVars::FABRO_CONFIG, "/tmp/settings.toml"),
                (EnvVars::FABRO_STORAGE_ROOT, "/tmp/storage"),
                (EnvVars::FABRO_DEV_TOKEN, "dev-token"),
                (EnvVars::SESSION_SECRET, "session-secret"),
            ])
            .get(key)
            .map(ToString::to_string)
        });

        assert!(env.contains(&"RUST_LOG=trace".to_string()));
        assert!(env.contains(&"RUST_BACKTRACE=1".to_string()));
        assert!(env.contains(&"FABRO_LOG=debug".to_string()));
        assert!(env.contains(&"FABRO_LOG_DESTINATION=file".to_string()));
        assert!(env.contains(&"FABRO_WORKER_TOKEN=worker-token".to_string()));
        assert!(env.contains(&"TERM=xterm-256color".to_string()));
        assert!(env.contains(&"NO_COLOR=1".to_string()));
        assert!(!env.iter().any(|entry| entry.starts_with("FABRO_CONFIG=")));
        assert!(
            !env.iter()
                .any(|entry| entry.starts_with("FABRO_STORAGE_ROOT="))
        );
        assert!(
            !env.iter()
                .any(|entry| entry.starts_with("FABRO_DEV_TOKEN="))
        );
        assert!(!env.iter().any(|entry| entry.starts_with("SESSION_SECRET=")));
    }

    #[test]
    fn worker_runtime_docker_command_uses_api_bootstrap() {
        let spec = test_docker_spec();

        assert_eq!(docker_worker_command(&spec), vec![
            "fabro".to_string(),
            "__run-worker".to_string(),
            "--bootstrap".to_string(),
            "api".to_string(),
            "--server".to_string(),
            "http://fabro-server:3333".to_string(),
            "--storage-dir".to_string(),
            DOCKER_WORKER_STORAGE_DIR.to_string(),
            "--run-dir".to_string(),
            DOCKER_WORKER_RUN_DIR.to_string(),
            "--run-id".to_string(),
            spec.common.run_id.to_string(),
            "--mode".to_string(),
            "start".to_string(),
        ]);
    }

    #[test]
    fn worker_runtime_docker_socket_mount_is_optional() {
        let mut spec = test_docker_spec();
        assert_eq!(docker_socket_bind(&spec), None);

        spec.docker_socket = Some(PathBuf::from("/var/run/custom-docker.sock"));

        assert_eq!(
            docker_socket_bind(&spec),
            Some(vec![
                "/var/run/custom-docker.sock:/var/run/docker.sock".to_string()
            ])
        );
    }

    #[test]
    fn worker_runtime_docker_container_names_have_unique_suffixes() {
        let run_id = RunId::new();

        let first = docker_worker_container_name(&run_id);
        let second = docker_worker_container_name(&run_id);

        assert!(first.starts_with(&format!("fabro-worker-{run_id}-")));
        assert!(second.starts_with(&format!("fabro-worker-{run_id}-")));
        assert_ne!(first, second);
    }

    #[test]
    fn worker_runtime_docker_log_output_preserves_stream_kind() {
        let stdout = worker_lines_from_log_output(LogOutput::StdOut {
            message: "one\ntwo\n".into(),
        });
        let stderr = worker_lines_from_log_output(LogOutput::StdErr {
            message: "bad\n".into(),
        });

        assert_eq!(stdout, vec![
            WorkerOutputLine {
                stream: WorkerOutputStreamKind::Stdout,
                line:   "one".to_string(),
            },
            WorkerOutputLine {
                stream: WorkerOutputStreamKind::Stdout,
                line:   "two".to_string(),
            },
        ]);
        assert_eq!(stderr, vec![WorkerOutputLine {
            stream: WorkerOutputStreamKind::Stderr,
            line:   "bad".to_string(),
        }]);
    }
}
