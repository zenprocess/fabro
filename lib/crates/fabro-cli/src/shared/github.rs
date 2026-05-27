use anyhow::anyhow;
use fabro_github::GitHubCredentials;
use fabro_static::EnvVars;
use fabro_types::settings::server::GithubIntegrationStrategy;
use fabro_vault::SecretStore;

pub(crate) async fn build_github_credentials(
    strategy: GithubIntegrationStrategy,
    app_id: Option<&str>,
    app_slug: Option<&str>,
    secrets: Option<&SecretStore>,
) -> anyhow::Result<Option<GitHubCredentials>> {
    match strategy {
        GithubIntegrationStrategy::App => {
            GitHubCredentials::from_env_with_slug(app_id, app_slug).map_err(|err| anyhow!(err))
        }
        GithubIntegrationStrategy::Token => {
            let token = lookup_github_token(secrets).await;
            match token {
                Some(t) => {
                    fabro_github::validate_static_github_token(&t)?;
                    Ok(Some(GitHubCredentials::Pat(t)))
                }
                None => Err(anyhow!(
                    "GITHUB_TOKEN not configured — run fabro install or set GITHUB_TOKEN"
                )),
            }
        }
    }
}

/// Look up GitHub token: GITHUB_TOKEN env -> secret store GITHUB_TOKEN -> GH_TOKEN env
/// -> secret store GH_TOKEN
async fn lookup_github_token(secrets: Option<&SecretStore>) -> Option<String> {
    match lookup_env_or_secret(EnvVars::GITHUB_TOKEN, secrets).await {
        Some(token) => Some(token),
        None => lookup_env_or_secret(EnvVars::GH_TOKEN, secrets).await,
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "GitHub credential resolution intentionally falls back from secret store to documented process-env names."
)]
async fn lookup_env_or_secret(name: &str, secrets: Option<&SecretStore>) -> Option<String> {
    let from_env = std::env::var(name).ok();
    let value = match from_env {
        Some(value) => Some(value),
        None => match secrets {
            Some(secrets) => secrets.get(name).await,
            None => None,
        },
    };
    value
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}
