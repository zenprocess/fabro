use std::sync::Arc;

use async_trait::async_trait;
use fabro_model::{Catalog, ProviderId};
use fabro_vault::SecretStore;

use crate::credential_source::{CredentialSource, ResolvedCredentials};
use crate::{CredentialResolver, CredentialUsage, EnvLookup, ResolveError, ResolvedCredential};

#[derive(Clone)]
pub struct SecretCredentialSource {
    resolver: CredentialResolver,
}

impl SecretCredentialSource {
    #[must_use]
    pub fn new(secrets: Arc<SecretStore>) -> Self {
        let resolver = CredentialResolver::new(secrets);
        Self { resolver }
    }

    #[must_use]
    pub fn with_env_lookup<F>(secrets: Arc<SecretStore>, env_lookup: F) -> Self
    where
        F: Fn(&str) -> Option<String> + Send + Sync + 'static,
    {
        let env_lookup: EnvLookup = Arc::new(env_lookup);
        let resolver = CredentialResolver::with_env_lookup(secrets, env_lookup);
        Self { resolver }
    }

    #[must_use]
    pub fn secrets_only(secrets: Arc<SecretStore>) -> Self {
        Self::with_env_lookup(secrets, |_| None)
    }
}

impl std::fmt::Debug for SecretCredentialSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretCredentialSource")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl CredentialSource for SecretCredentialSource {
    async fn resolve(&self, catalog: &Catalog) -> anyhow::Result<ResolvedCredentials> {
        let mut credentials = Vec::new();
        let mut auth_issues = Vec::new();

        for provider in catalog.providers() {
            match self
                .resolver
                .resolve(provider.id.clone(), CredentialUsage::ApiRequest, catalog)
                .await
            {
                Ok(ResolvedCredential::Api(credential)) => credentials.push(credential),
                Err(ResolveError::NotConfigured(_)) if provider.auth.is_some() => {}
                Err(err) => auth_issues.push((provider.id.clone(), err)),
            }
        }

        Ok(ResolvedCredentials {
            credentials,
            auth_issues,
        })
    }

    async fn configured_providers(&self, catalog: &Catalog) -> Vec<ProviderId> {
        self.resolver.configured_providers(catalog).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use fabro_model::{Catalog, ProviderId};
    use fabro_vault::SecretStore;

    use super::SecretCredentialSource;
    use crate::credential::{OAuthConfig, OAuthCredential, OAuthTokens};
    use crate::vault_ext::{secret_set_oauth, secret_set_token};
    use crate::{CredentialSource, ResolveError};

    fn expired_openai_credential() -> OAuthCredential {
        OAuthCredential {
            tokens:     OAuthTokens {
                access_token:  "expired-access".to_string(),
                refresh_token: Some("refresh-token".to_string()),
                expires_at:    Utc::now() - Duration::hours(1),
            },
            config:     OAuthConfig {
                auth_url:     "https://auth.openai.com".to_string(),
                token_url:    "http://127.0.0.1:9/oauth/token".to_string(),
                client_id:    "client".to_string(),
                scopes:       vec!["openid".to_string()],
                redirect_uri: Some("https://example.com/callback".to_string()),
                use_pkce:     true,
            },
            account_id: Some("acct_123".to_string()),
        }
    }

    fn default_catalog() -> Catalog {
        Catalog::from_builtin().unwrap()
    }

    #[tokio::test]
    async fn resolve_returns_credentials_and_auth_issues() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_oauth(
            &secrets,
            crate::OPENAI_CODEX_VAULT_SECRET_NAME,
            &expired_openai_credential(),
        )
        .await
        .unwrap();
        secret_set_token(&secrets, "ANTHROPIC_API_KEY", "anthropic-key")
            .await
            .unwrap();

        let source = SecretCredentialSource::with_env_lookup(Arc::new(secrets), |_| None);
        let catalog = default_catalog();

        let resolved = source.resolve(&catalog).await.unwrap();

        assert_eq!(resolved.credentials.len(), 1);
        assert_eq!(resolved.credentials[0].provider, ProviderId::anthropic());
        assert_eq!(resolved.auth_issues.len(), 1);
        assert!(matches!(
            &resolved.auth_issues[0].1,
            ResolveError::RefreshFailed {
                provider,
                ..
            } if provider == &ProviderId::openai()
        ));
    }

    #[tokio::test]
    async fn configured_providers_reads_from_vault_without_refreshing() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_token(&secrets, "OPENAI_API_KEY", "openai-key")
            .await
            .unwrap();
        secret_set_token(&secrets, "ANTHROPIC_API_KEY", "anthropic-key")
            .await
            .unwrap();
        let source = SecretCredentialSource::with_env_lookup(Arc::new(secrets), |_| None);
        let catalog = default_catalog();

        assert_eq!(source.configured_providers(&catalog).await, vec![
            ProviderId::anthropic(),
            ProviderId::openai()
        ]);
    }

    #[tokio::test]
    async fn secrets_only_ignores_env_lookup_values() {
        let env_dir = tempfile::tempdir().unwrap();
        let secrets_only_dir = tempfile::tempdir().unwrap();
        let catalog = default_catalog();
        let env_backed = SecretCredentialSource::with_env_lookup(
            Arc::new(
                SecretStore::load(env_dir.path().join("secrets.json"))
                    .await
                    .unwrap(),
            ),
            |name| (name == "OPENAI_API_KEY").then(|| "env-openai-key".to_string()),
        );
        assert_eq!(env_backed.configured_providers(&catalog).await, vec![
            ProviderId::openai()
        ]);

        let secrets_only = SecretCredentialSource::secrets_only(Arc::new(
            SecretStore::load(secrets_only_dir.path().join("secrets.json"))
                .await
                .unwrap(),
        ));

        assert!(
            secrets_only.configured_providers(&catalog).await.is_empty(),
            "secrets_only must not resolve env-backed provider keys"
        );
        let resolved = secrets_only.resolve(&catalog).await.unwrap();
        assert!(resolved.credentials.is_empty());
        assert!(resolved.auth_issues.is_empty());
    }
}
