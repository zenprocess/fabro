use anyhow::anyhow;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use fabro_github::{GitHubAppCredentials, GitHubCredentials};
use fabro_static::EnvVars;
use fabro_types::settings::server::GithubIntegrationStrategy;
use fabro_vault::Vault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitHubCredentialLookup {
    Local,
    ApiBootstrapVault,
}

pub(crate) fn build_github_credentials(
    strategy: GithubIntegrationStrategy,
    app_id: Option<&str>,
    app_slug: Option<&str>,
    vault: Option<&Vault>,
    lookup: GitHubCredentialLookup,
) -> anyhow::Result<Option<GitHubCredentials>> {
    match strategy {
        GithubIntegrationStrategy::App => match lookup {
            GitHubCredentialLookup::Local => {
                GitHubCredentials::from_env_with_slug(app_id, app_slug).map_err(|err| anyhow!(err))
            }
            GitHubCredentialLookup::ApiBootstrapVault => {
                build_github_app_credentials_from_vault(app_id, app_slug, vault)
            }
        },
        GithubIntegrationStrategy::Token => {
            let token = match lookup {
                GitHubCredentialLookup::Local => lookup_github_token(vault),
                GitHubCredentialLookup::ApiBootstrapVault => {
                    lookup_vault_secret(EnvVars::GITHUB_TOKEN, vault)
                }
            };
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

fn build_github_app_credentials_from_vault(
    app_id: Option<&str>,
    app_slug: Option<&str>,
    vault: Option<&Vault>,
) -> anyhow::Result<Option<GitHubCredentials>> {
    let Some(app_id) = app_id else {
        return Ok(None);
    };
    let raw = lookup_vault_secret(EnvVars::GITHUB_APP_PRIVATE_KEY, vault).ok_or_else(|| {
        anyhow!("GITHUB_APP_PRIVATE_KEY is missing from the worker bootstrap vault")
    })?;
    let private_key_pem =
        decode_pem_value(EnvVars::GITHUB_APP_PRIVATE_KEY, &raw).map_err(anyhow::Error::msg)?;
    Ok(Some(GitHubCredentials::App(GitHubAppCredentials {
        app_id: app_id.to_string(),
        private_key_pem,
        slug: app_slug
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .map(str::to_string),
    })))
}

fn decode_pem_value(name: &str, raw: &str) -> Result<String, String> {
    if raw.starts_with("-----") {
        return Ok(raw.to_string());
    }
    let pem_bytes = BASE64_STANDARD
        .decode(raw)
        .map_err(|err| format!("{name} is not valid PEM or base64: {err}"))?;
    String::from_utf8(pem_bytes)
        .map_err(|err| format!("{name} base64 decoded to invalid UTF-8: {err}"))
}

/// Look up GitHub token: GITHUB_TOKEN env -> vault GITHUB_TOKEN -> GH_TOKEN env
/// -> vault GH_TOKEN
fn lookup_github_token(vault: Option<&Vault>) -> Option<String> {
    lookup_env_or_vault(EnvVars::GITHUB_TOKEN, vault)
        .or_else(|| lookup_env_or_vault(EnvVars::GH_TOKEN, vault))
}

fn lookup_vault_secret(name: &str, vault: Option<&Vault>) -> Option<String> {
    vault
        .and_then(|v| v.get(name).map(str::to_string))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

#[expect(
    clippy::disallowed_methods,
    reason = "GitHub credential resolution intentionally falls back from vault to documented process-env names."
)]
fn lookup_env_or_vault(name: &str, vault: Option<&Vault>) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| vault.and_then(|v| v.get(name).map(str::to_string)))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}
