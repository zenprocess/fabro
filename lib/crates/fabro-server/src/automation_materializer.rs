use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use fabro_api::types::RunManifest;
use fabro_automation::AutomationTarget;
use fabro_config::Storage;
use fabro_redact::DisplaySafeUrl;
use fabro_sandbox::redact::redact_auth_url;
use fabro_types::RunId;
use tokio::process::Command;
use tokio::time::timeout;
use tokio::{fs, task};

pub(crate) struct AutomationRunMaterializeInput {
    pub target:             AutomationTarget,
    pub run_id:             RunId,
    pub user_settings_path: PathBuf,
    pub temp_root:          PathBuf,
}

#[derive(Clone)]
pub(crate) struct AutomationRunMaterialized {
    pub manifest:                 RunManifest,
    pub submitted_manifest_bytes: Vec<u8>,
}

#[derive(thiserror::Error, Debug, Clone)]
pub(crate) enum AutomationRunMaterializeError {
    #[error("invalid automation target: {0}")]
    InvalidTarget(String),
    #[error("failed to clone automation repository: {0}")]
    CloneFailed(String),
    #[error("failed to build run manifest: {0}")]
    Manifest(String),
}

#[async_trait]
pub(crate) trait AutomationRunMaterializer: Send + Sync {
    async fn materialize(
        &self,
        input: AutomationRunMaterializeInput,
    ) -> Result<AutomationRunMaterialized, AutomationRunMaterializeError>;
}

pub(crate) struct GitAutomationRunMaterializer {
    github_credentials:  Option<fabro_github::GitHubCredentials>,
    github_api_base_url: String,
    http_client:         Option<fabro_http::HttpClient>,
    git_timeout:         Duration,
}

impl GitAutomationRunMaterializer {
    pub(crate) fn new(
        github_credentials: Option<fabro_github::GitHubCredentials>,
        github_api_base_url: String,
        http_client: Option<fabro_http::HttpClient>,
    ) -> Self {
        Self {
            github_credentials,
            github_api_base_url,
            http_client,
            git_timeout: Duration::from_mins(2),
        }
    }
}

#[async_trait]
impl AutomationRunMaterializer for GitAutomationRunMaterializer {
    async fn materialize(
        &self,
        input: AutomationRunMaterializeInput,
    ) -> Result<AutomationRunMaterialized, AutomationRunMaterializeError> {
        let (owner, repo) = input.target.repository.owner_repo();
        if owner.is_empty() || repo.is_empty() {
            return Err(AutomationRunMaterializeError::InvalidTarget(
                input.target.repository.to_string(),
            ));
        }
        let sanitized_clone_url = github_clone_url(owner, repo);
        let auth_url = self.authenticated_clone_url(&sanitized_clone_url).await?;
        let clone_url = auth_url
            .as_ref()
            .map_or_else(|| sanitized_clone_url.clone(), DisplaySafeUrl::raw_string);

        fs::create_dir_all(&input.temp_root).await.map_err(|err| {
            AutomationRunMaterializeError::CloneFailed(format!(
                "failed to create temp root {}: {err}",
                input.temp_root.display()
            ))
        })?;
        let checkout_dir = input.temp_root.join(input.run_id.to_string());
        let result = self
            .run_checkout(
                &input,
                &checkout_dir,
                &clone_url,
                &sanitized_clone_url,
                auth_url.as_ref(),
            )
            .await;
        // Always clean up the materialized clone: callers don't need the
        // working tree after the manifest is built, and a failed clone
        // (e.g. partial fetch) should not leak gigabytes into scratch.
        if let Err(err) = fs::remove_dir_all(&checkout_dir).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    error = %err,
                    path = %checkout_dir.display(),
                    "Failed to clean up automation checkout",
                );
            }
        }
        result
    }
}

impl GitAutomationRunMaterializer {
    async fn authenticated_clone_url(
        &self,
        sanitized_clone_url: &str,
    ) -> Result<Option<DisplaySafeUrl>, AutomationRunMaterializeError> {
        let Some(credentials) = self.github_credentials.as_ref() else {
            return Ok(None);
        };
        let ctx = match self.http_client.clone() {
            Some(client) => fabro_github::GitHubContext::with_http_client(
                credentials,
                &self.github_api_base_url,
                client,
            ),
            None => fabro_github::GitHubContext::new(credentials, &self.github_api_base_url),
        };
        fabro_github::resolve_authenticated_url(&ctx, sanitized_clone_url)
            .await
            .map(Some)
            .map_err(|err| AutomationRunMaterializeError::CloneFailed(err.to_string()))
    }

    async fn run_checkout(
        &self,
        input: &AutomationRunMaterializeInput,
        checkout_dir: &Path,
        clone_url: &str,
        sanitized_clone_url: &str,
        auth_url: Option<&DisplaySafeUrl>,
    ) -> Result<AutomationRunMaterialized, AutomationRunMaterializeError> {
        run_git(
            git_clone_args(clone_url, checkout_dir),
            self.git_timeout,
            "git clone",
            auth_url,
        )
        .await?;
        run_git(
            git_remote_set_url_args(checkout_dir, sanitized_clone_url),
            self.git_timeout,
            "git remote set-url origin",
            auth_url,
        )
        .await?;
        run_git(
            git_checkout_args(checkout_dir, input.target.ref_.as_str()),
            self.git_timeout,
            "git checkout",
            auth_url,
        )
        .await?;
        build_manifest_from_checkout(input, checkout_dir).await
    }
}

pub(crate) fn automation_temp_root(storage_root: impl Into<PathBuf>) -> PathBuf {
    Storage::new(storage_root).scratch_dir().join("automations")
}

fn github_clone_url(owner: &str, repo: &str) -> String {
    format!("https://github.com/{owner}/{repo}.git")
}

fn git_clone_args(clone_url: &str, checkout_path: &Path) -> Vec<OsString> {
    vec![
        "clone".into(),
        "--no-tags".into(),
        "--".into(),
        clone_url.into(),
        checkout_path.as_os_str().to_owned(),
    ]
}

fn git_remote_set_url_args(repo_dir: &Path, sanitized_clone_url: &str) -> Vec<OsString> {
    vec![
        "-C".into(),
        repo_dir.as_os_str().to_owned(),
        "remote".into(),
        "set-url".into(),
        "origin".into(),
        sanitized_clone_url.into(),
    ]
}

fn git_checkout_args(repo_dir: &Path, ref_: &str) -> Vec<OsString> {
    vec![
        "-C".into(),
        repo_dir.as_os_str().to_owned(),
        "checkout".into(),
        "--force".into(),
        ref_.into(),
    ]
}

async fn run_git(
    args: Vec<OsString>,
    git_timeout: Duration,
    label: &'static str,
    auth_url: Option<&DisplaySafeUrl>,
) -> Result<(), AutomationRunMaterializeError> {
    let mut command = Command::new("git");
    command.args(&args);
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.kill_on_drop(true);
    let output = timeout(git_timeout, command.output())
        .await
        .map_err(|_| {
            AutomationRunMaterializeError::CloneFailed(format!(
                "{label} timed out after {}s",
                git_timeout.as_secs()
            ))
        })?
        .map_err(|err| {
            AutomationRunMaterializeError::CloneFailed(redact_auth_url(
                &format!("{label}: {err}"),
                auth_url,
            ))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    Err(AutomationRunMaterializeError::CloneFailed(redact_auth_url(
        &format!("{label} exited with status {}: {detail}", output.status),
        auth_url,
    )))
}

async fn build_manifest_from_checkout(
    input: &AutomationRunMaterializeInput,
    checkout_dir: &Path,
) -> Result<AutomationRunMaterialized, AutomationRunMaterializeError> {
    let workflow = PathBuf::from(input.target.workflow.as_str());
    let user_settings_path = input.user_settings_path.clone();
    let run_id = input.run_id;
    let cwd = checkout_dir.to_path_buf();
    let built = task::spawn_blocking(move || {
        fabro_manifest::build_run_manifest(fabro_manifest::ManifestBuildInput {
            workflow,
            cwd,
            run_id: Some(run_id),
            user_settings_path: Some(user_settings_path),
            ..fabro_manifest::ManifestBuildInput::default()
        })
    })
    .await
    .map_err(|err| AutomationRunMaterializeError::Manifest(err.to_string()))?
    .map_err(|err| AutomationRunMaterializeError::Manifest(err.to_string()))?;
    let submitted_manifest_bytes = serde_json::to_vec(&built.manifest)
        .map_err(|err| AutomationRunMaterializeError::Manifest(err.to_string()))?;
    Ok(AutomationRunMaterialized {
        manifest: built.manifest,
        submitted_manifest_bytes,
    })
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct StaticAutomationRunMaterializer {
    result: Result<AutomationRunMaterialized, AutomationRunMaterializeError>,
}

#[cfg(any(test, feature = "test-support"))]
impl StaticAutomationRunMaterializer {
    pub(crate) fn ok(
        manifest: RunManifest,
        submitted_manifest_bytes: Vec<u8>,
    ) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            result: Ok(AutomationRunMaterialized {
                manifest,
                submitted_manifest_bytes,
            }),
        })
    }
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait]
impl AutomationRunMaterializer for StaticAutomationRunMaterializer {
    async fn materialize(
        &self,
        _input: AutomationRunMaterializeInput,
    ) -> Result<AutomationRunMaterialized, AutomationRunMaterializeError> {
        self.result.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use fabro_automation::{GitRefSelector, RepositorySlug, WorkflowSlug};

    use super::*;

    #[test]
    fn github_clone_url_uses_sanitized_https_origin() {
        assert_eq!(
            github_clone_url("fabro-sh", "fabro"),
            "https://github.com/fabro-sh/fabro.git"
        );
    }

    #[test]
    fn redact_auth_url_strips_credentials_from_stderr() {
        let auth_url =
            DisplaySafeUrl::parse("https://x-access-token:ghs_secret@github.com/acme/widgets.git")
                .expect("auth url should parse");
        let redacted = redact_auth_url(
            "fatal: https://x-access-token:ghs_secret@github.com/acme/widgets.git\nremote: denied",
            Some(&auth_url),
        );
        assert!(!redacted.contains("ghs_secret"));
        // Newlines are preserved (unlike a whitespace-collapse redactor).
        assert!(redacted.contains('\n'));
    }

    #[test]
    fn checkout_args_pass_ref_as_argv() {
        let args = git_checkout_args(Path::new("/tmp/repo"), "feature/main");
        assert_eq!(args[0], OsString::from("-C"));
        assert_eq!(args[2], OsString::from("checkout"));
        assert_eq!(args[4], OsString::from("feature/main"));
    }

    #[tokio::test]
    async fn build_manifest_from_checkout_resolves_workflow_path() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let workflow_dir = dir.path().join("flows");
        fs::create_dir_all(&workflow_dir)
            .await
            .expect("workflow dir should be created");
        fs::write(
            workflow_dir.join("deps.fabro"),
            r#"digraph Test {
    graph [goal="Test"]
    start [shape=Mdiamond]
    exit [shape=Msquare]
    start -> exit
}"#,
        )
        .await
        .expect("workflow should be written");
        let target = AutomationTarget {
            repository: RepositorySlug::from_str("fabro-sh/fabro").unwrap(),
            ref_:       GitRefSelector::from_str("main").unwrap(),
            workflow:   WorkflowSlug::from_str("flows/deps").unwrap(),
        };
        let run_id = RunId::new();
        let input = AutomationRunMaterializeInput {
            target,
            run_id,
            user_settings_path: dir.path().join("settings.toml"),
            temp_root: dir.path().join("tmp"),
        };

        let materialized = build_manifest_from_checkout(&input, dir.path())
            .await
            .expect("manifest should build");

        assert_eq!(
            materialized.manifest.run_id.as_deref(),
            Some(run_id.to_string().as_str())
        );
        assert_eq!(materialized.manifest.target.path, "flows/deps.fabro");
        assert!(
            std::str::from_utf8(&materialized.submitted_manifest_bytes)
                .unwrap()
                .contains("flows/deps.fabro")
        );
    }
}
