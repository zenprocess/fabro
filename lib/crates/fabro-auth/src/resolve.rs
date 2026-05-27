use std::collections::HashMap;
use std::sync::Arc;

use fabro_model::catalog::CatalogProvider;
use fabro_model::{ApiKeyHeaderPolicy, Catalog, CredentialRef, HeaderValueRef, ProviderId};
use fabro_static::EnvVars;
use fabro_vault::{SecretStore, SecretType};

use crate::credential::{ApiKeyHeader, OAuthCredential};
use crate::credential_source::CredentialSource;
use crate::env_source::EnvCredentialSource;
use crate::refresh::refresh_oauth_credential;
use crate::vault_ext::{
    SecretLookupError, secret_get_oauth, secret_get_token, secret_set_oauth,
};

pub type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialUsage {
    ApiRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedSecret {
    ApiKey(String),
    OAuth {
        credential: Box<OAuthCredential>,
        vault_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCredential {
    pub provider:      ProviderId,
    pub auth_header:   Option<ApiKeyHeader>,
    pub extra_headers: HashMap<String, String>,
    pub base_url:      Option<String>,
    pub codex_mode:    bool,
    pub org_id:        Option<String>,
    pub project_id:    Option<String>,
}

impl ApiCredential {
    /// Build an `ApiCredential` from an API key using the supplied catalog for
    /// auth header policy and provider base URL.
    pub fn from_api_key(
        provider: impl Into<ProviderId>,
        key: String,
        catalog: &Catalog,
    ) -> Result<Self, ResolveError> {
        let provider_id = provider.into();
        let provider = catalog
            .provider(&provider_id)
            .ok_or_else(|| ResolveError::NotConfigured(provider_id.clone()))?;
        let auth_header = auth_header_for_catalog_provider(provider, key)?;
        Ok(Self {
            provider:      provider_id,
            auth_header:   Some(auth_header),
            extra_headers: HashMap::new(),
            base_url:      provider.base_url.clone(),
            codex_mode:    false,
            org_id:        None,
            project_id:    None,
        })
    }
}

const OPENAI_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const CHATGPT_ACCOUNT_ID_HEADER: &str = "ChatGPT-Account-Id";
const ORIGINATOR_HEADER: &str = "originator";
const FABRO_ORIGINATOR: &str = "fabro";

pub(crate) fn apply_openai_api_env_context(
    credential: &mut ApiCredential,
    env_lookup: &(dyn Fn(&str) -> Option<String> + Send + Sync),
) {
    credential.org_id = env_lookup(EnvVars::OPENAI_ORG_ID);
    credential.project_id = env_lookup(EnvVars::OPENAI_PROJECT_ID);
}

pub(crate) fn apply_openai_codex_api_context(
    credential: &mut ApiCredential,
    account_id: Option<&str>,
    env_lookup: &(dyn Fn(&str) -> Option<String> + Send + Sync),
) {
    apply_openai_api_env_context(credential, env_lookup);
    if let Some(account_id) = account_id {
        credential.extra_headers.insert(
            CHATGPT_ACCOUNT_ID_HEADER.to_string(),
            account_id.to_string(),
        );
    }
    credential
        .extra_headers
        .insert(ORIGINATOR_HEADER.to_string(), FABRO_ORIGINATOR.to_string());
    credential.base_url = Some(OPENAI_CODEX_BASE_URL.to_string());
    credential.codex_mode = true;
}

#[must_use]
pub fn build_api_key_header(policy: ApiKeyHeaderPolicy, key: String) -> ApiKeyHeader {
    match policy {
        ApiKeyHeaderPolicy::Bearer => ApiKeyHeader::Bearer(key),
        ApiKeyHeaderPolicy::Custom { name } => ApiKeyHeader::Custom { name, value: key },
    }
}

fn auth_header_for_catalog_provider(
    provider: &CatalogProvider,
    key: String,
) -> Result<ApiKeyHeader, ResolveError> {
    let Some(auth) = &provider.auth else {
        return Err(ResolveError::NotConfigured(provider.id.clone()));
    };
    Ok(build_api_key_header(auth.header.clone(), key))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCredential {
    Api(ApiCredential),
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("{0} is not configured")]
    NotConfigured(ProviderId),
    #[error("{provider} secret credential '{name}' has schema {actual:?}, expected Token or Oauth")]
    SecretSchemaMismatch {
        provider: ProviderId,
        name:     String,
        actual:   SecretType,
    },
    #[error("{provider} secret credential '{name}' is not valid Oauth JSON: {source}")]
    SecretDecodeFailed {
        provider: ProviderId,
        name:     String,
        #[source]
        source:   serde_json::Error,
    },
    #[error("{provider} requires re-authentication: {source}")]
    RefreshFailed {
        provider: ProviderId,
        #[source]
        source:   anyhow::Error,
    },
    #[error("{0} requires re-authentication: missing refresh token")]
    RefreshTokenMissing(ProviderId),
}

#[must_use]
pub fn auth_issue_message(provider: &ProviderId, err: &ResolveError) -> String {
    let provider_name = provider.display_name();
    match err {
        ResolveError::NotConfigured(_) => {
            format!("{provider_name} is not configured")
        }
        ResolveError::SecretSchemaMismatch { name, actual, .. } => {
            format!(
                "{provider_name} secret credential '{name}' has schema {actual:?}, expected Token or Oauth"
            )
        }
        ResolveError::SecretDecodeFailed { name, source, .. } => {
            format!("{provider_name} secret credential '{name}' is not valid OAuth JSON: {source}")
        }
        ResolveError::RefreshFailed { source, .. } => {
            format!("{provider_name} requires re-authentication: {source}")
        }
        ResolveError::RefreshTokenMissing(_) => {
            format!("{provider_name} requires re-authentication: refresh token missing")
        }
    }
}

#[derive(Clone)]
pub struct CredentialResolver {
    secrets:   Arc<SecretStore>,
    env_lookup: EnvLookup,
}

impl CredentialResolver {
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "CredentialResolver owns the process-env fallback used after vault lookup."
    )]
    pub fn new(secrets: Arc<SecretStore>) -> Self {
        Self::with_env_lookup(secrets, Arc::new(|name| std::env::var(name).ok()))
    }

    #[must_use]
    pub fn with_env_lookup(secrets: Arc<SecretStore>, env_lookup: EnvLookup) -> Self {
        Self {
            secrets,
            env_lookup,
        }
    }

    pub async fn resolve(
        &self,
        provider: impl Into<ProviderId>,
        _usage: CredentialUsage,
        catalog: &Catalog,
    ) -> Result<ResolvedCredential, ResolveError> {
        let provider_id = provider.into();
        let Some(catalog_provider) = catalog.provider(&provider_id) else {
            return Err(ResolveError::NotConfigured(provider_id));
        };
        if catalog_provider.auth.is_none() {
            return self
                .api_credential_from_provider_auth(catalog_provider, catalog)
                .await
                .map(ResolvedCredential::Api);
        }
        let initial_secret = self.find_credential(catalog_provider).await?;

        let secret = if let ResolvedSecret::OAuth {
            credential,
            vault_name,
        } = &initial_secret
        {
            if !credential.needs_refresh() {
                initial_secret
            } else if credential.tokens.refresh_token.is_none() {
                return Err(ResolveError::RefreshTokenMissing(provider_id.clone()));
            } else {
                let refreshed = refresh_oauth_credential(credential)
                    .await
                    .map_err(|source| ResolveError::RefreshFailed {
                        provider: provider_id.clone(),
                        source,
                    })?;
                let refreshed_for_store = refreshed.clone();
                let vault_name_for_store = vault_name.clone();
                secret_set_oauth(&self.secrets, &vault_name_for_store, &refreshed_for_store)
                    .await
                    .map(|_| ())
                    .map_err(|source| ResolveError::RefreshFailed {
                        provider: provider_id.clone(),
                        source:   anyhow::Error::from(source),
                    })?;
                ResolvedSecret::OAuth {
                    credential: Box::new(refreshed),
                    vault_name: vault_name.clone(),
                }
            }
        } else {
            initial_secret
        };

        self.to_api_credential(&provider_id, &secret, catalog)
            .await
            .map(ResolvedCredential::Api)
    }

    pub async fn configured_providers(&self, catalog: &Catalog) -> Vec<ProviderId> {
        let mut providers = Vec::new();
        for provider in catalog.providers() {
            if self.has_credential_material(provider, catalog).await {
                providers.push(provider.id.clone());
            }
        }
        providers
    }

    async fn find_credential(
        &self,
        provider: &CatalogProvider,
    ) -> Result<ResolvedSecret, ResolveError> {
        let Some(auth) = &provider.auth else {
            return Err(ResolveError::NotConfigured(provider.id.clone()));
        };

        for credential_ref in &auth.credentials {
            if let Some(credential) =
                self.credential_from_ref(&provider.id, credential_ref).await?
            {
                return Ok(credential);
            }
        }

        Err(ResolveError::NotConfigured(provider.id.clone()))
    }

    async fn has_credential_material(
        &self,
        provider: &CatalogProvider,
        catalog: &Catalog,
    ) -> bool {
        let Some(auth) = &provider.auth else {
            return self
                .resolved_extra_headers_for_catalog(&provider.id, catalog)
                .await
                .is_ok();
        };
        for credential_ref in &auth.credentials {
            if self
                .credential_from_ref(&provider.id, credential_ref)
                .await
                .is_ok_and(|credential| credential.is_some())
            {
                return true;
            }
        }
        false
    }

    async fn credential_from_ref(
        &self,
        provider: &ProviderId,
        credential_ref: &CredentialRef,
    ) -> Result<Option<ResolvedSecret>, ResolveError> {
        match credential_ref {
            CredentialRef::Vault(name) => match secret_get_token(&self.secrets, name).await {
                Ok(Some(token)) => Ok(Some(ResolvedSecret::ApiKey(token))),
                Ok(None) => Ok(None),
                Err(SecretLookupError::SchemaMismatch {
                    actual: SecretType::Oauth,
                    ..
                }) => secret_get_oauth(&self.secrets, name)
                    .await
                    .map(|credential| {
                        credential.map(|credential| ResolvedSecret::OAuth {
                            credential: Box::new(credential),
                            vault_name: name.clone(),
                        })
                    })
                    .map_err(|err| vault_lookup_error(provider, name, err)),
                Err(err) => Err(vault_lookup_error(provider, name, err)),
            },
            CredentialRef::Env(name) => Ok((self.env_lookup)(name).map(ResolvedSecret::ApiKey)),
        }
    }

    fn lookup_env(&self, name: &str) -> Option<String> {
        (self.env_lookup)(name)
    }

    fn provider_base_url_for_catalog(provider: &ProviderId, catalog: &Catalog) -> Option<String> {
        catalog
            .provider(provider)
            .and_then(|provider| provider.base_url.clone())
    }

    async fn resolved_extra_headers_for_catalog(
        &self,
        provider: &ProviderId,
        catalog: &Catalog,
    ) -> Result<HashMap<String, String>, ResolveError> {
        let Some(catalog_provider) = catalog.provider(provider) else {
            return Ok(HashMap::new());
        };
        let mut headers = HashMap::new();
        for (name, value_ref) in &catalog_provider.extra_headers {
            let value = match value_ref {
                HeaderValueRef::Literal(value) => Some(value.clone()),
                HeaderValueRef::Env(name) => self.lookup_env(name),
                HeaderValueRef::Vault(name) => self.secrets.get(name).await,
            }
            .ok_or_else(|| ResolveError::NotConfigured(provider.clone()))?;
            headers.insert(name.clone(), value);
        }
        Ok(headers)
    }

    async fn to_api_credential(
        &self,
        provider_id: &ProviderId,
        secret: &ResolvedSecret,
        catalog: &Catalog,
    ) -> Result<ApiCredential, ResolveError> {
        let base_url = Self::provider_base_url_for_catalog(provider_id, catalog);
        match secret {
            ResolvedSecret::ApiKey(key) => {
                let provider = catalog
                    .provider(provider_id)
                    .ok_or_else(|| ResolveError::NotConfigured(provider_id.clone()))?;
                let auth_header = auth_header_for_catalog_provider(provider, key.clone())?;
                let mut cred = ApiCredential {
                    provider:      provider_id.clone(),
                    auth_header:   Some(auth_header),
                    extra_headers: HashMap::new(),
                    base_url:      None,
                    codex_mode:    false,
                    org_id:        None,
                    project_id:    None,
                };
                cred.base_url = base_url;
                cred.extra_headers =
                    self.resolved_extra_headers_for_catalog(provider_id, catalog)
                        .await?;
                if provider_id == &ProviderId::openai() {
                    apply_openai_api_env_context(&mut cred, &*self.env_lookup);
                }
                Ok(cred)
            }
            ResolvedSecret::OAuth { credential, .. } => {
                let mut extra_headers =
                    self.resolved_extra_headers_for_catalog(provider_id, catalog)
                        .await?;
                let mut api_credential = ApiCredential {
                    provider: provider_id.clone(),
                    auth_header: Some(ApiKeyHeader::Bearer(credential.tokens.access_token.clone())),
                    extra_headers: std::mem::take(&mut extra_headers),
                    base_url,
                    codex_mode: false,
                    org_id: None,
                    project_id: None,
                };
                if provider_id == &ProviderId::openai() {
                    apply_openai_codex_api_context(
                        &mut api_credential,
                        credential.account_id.as_deref(),
                        &*self.env_lookup,
                    );
                }
                Ok(api_credential)
            }
        }
    }

    async fn api_credential_from_provider_auth(
        &self,
        provider: &CatalogProvider,
        catalog: &Catalog,
    ) -> Result<ApiCredential, ResolveError> {
        if provider.auth.is_some() {
            return Err(ResolveError::NotConfigured(provider.id.clone()));
        }
        let extra_headers =
            self.resolved_extra_headers_for_catalog(&provider.id, catalog)
                .await?;
        Ok(ApiCredential {
            provider: provider.id.clone(),
            auth_header: None,
            extra_headers,
            base_url: Self::provider_base_url_for_catalog(&provider.id, catalog),
            codex_mode: false,
            org_id: None,
            project_id: None,
        })
    }
}

fn vault_lookup_error(provider: &ProviderId, name: &str, err: SecretLookupError) -> ResolveError {
    match err {
        SecretLookupError::SchemaMismatch { actual, .. } => ResolveError::SecretSchemaMismatch {
            provider: provider.clone(),
            name: name.to_string(),
            actual,
        },
        SecretLookupError::DecodeFailed { source, .. } => ResolveError::SecretDecodeFailed {
            provider: provider.clone(),
            name: name.to_string(),
            source,
        },
    }
}

pub async fn configured_providers_from_process_env(
    secrets: Option<&Arc<SecretStore>>,
    catalog: &Catalog,
) -> Vec<ProviderId> {
    match secrets {
        Some(secrets) => CredentialResolver::new(Arc::clone(secrets))
            .configured_providers(catalog)
            .await,
        None => {
            EnvCredentialSource::new()
                .configured_providers(catalog)
                .await
        }
    }
}
#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use fabro_model::catalog::LlmCatalogSettings;
    use httpmock::Method::POST;
    use httpmock::MockServer;

    use super::*;
    use crate::credential::{OAuthConfig, OAuthCredential, OAuthTokens};
    use crate::vault_ext::{secret_get_oauth, secret_set_oauth, secret_set_token};

    fn oauth_credential(token_url: String, expires_at: chrono::DateTime<Utc>) -> OAuthCredential {
        OAuthCredential {
            tokens:     OAuthTokens {
                access_token: "expired-access".to_string(),
                refresh_token: Some("refresh-token".to_string()),
                expires_at,
            },
            config:     OAuthConfig {
                auth_url: "https://auth.openai.com".to_string(),
                token_url,
                client_id: "test-client".to_string(),
                scopes: vec!["openid".to_string()],
                redirect_uri: Some("https://auth.openai.com/deviceauth/callback".to_string()),
                use_pkce: true,
            },
            account_id: Some("acct_123".to_string()),
        }
    }

    fn test_resolver(secrets: SecretStore, env_lookup: EnvLookup) -> CredentialResolver {
        CredentialResolver::with_env_lookup(Arc::new(secrets), env_lookup)
    }

    fn catalog_with(overrides: &str) -> Catalog {
        let settings: LlmCatalogSettings = toml::from_str(overrides).unwrap();
        Catalog::from_builtin_with_overrides(&settings).unwrap()
    }

    fn default_catalog() -> Catalog {
        catalog_with("")
    }

    #[tokio::test]
    async fn resolve_openai_api_request_prefers_env_when_listed_first() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_token(&secrets, "OPENAI_API_KEY", "vault-key")
            .await
            .unwrap();
        let resolver = test_resolver(
            secrets,
            Arc::new(|name| (name == "OPENAI_API_KEY").then(|| "env-key".to_string())),
        );
        let catalog = default_catalog();

        let resolved = resolver
            .resolve(ProviderId::openai(), CredentialUsage::ApiRequest, &catalog)
            .await
            .unwrap();

        let ResolvedCredential::Api(api) = resolved;
        assert_eq!(
            api.auth_header,
            Some(ApiKeyHeader::Bearer("env-key".to_string()))
        );
    }

    #[tokio::test]
    async fn resolve_openai_api_request_falls_back_to_codex_oauth_credential() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_oauth(
            &secrets,
            crate::OPENAI_CODEX_VAULT_SECRET_NAME,
            &oauth_credential(
                "https://auth.openai.com/oauth/token".to_string(),
                Utc::now() + Duration::hours(1),
            ),
        )
        .await
        .unwrap();
        let resolver = test_resolver(secrets, Arc::new(|_| None));
        let catalog = default_catalog();

        let resolved = resolver
            .resolve(ProviderId::openai(), CredentialUsage::ApiRequest, &catalog)
            .await
            .unwrap();

        let ResolvedCredential::Api(api) = resolved;
        assert_eq!(
            api.auth_header,
            Some(ApiKeyHeader::Bearer("expired-access".to_string()))
        );
        assert!(api.codex_mode);
        assert_eq!(
            api.base_url.as_deref(),
            Some("https://chatgpt.com/backend-api/codex")
        );
    }

    #[tokio::test]
    async fn resolve_returns_not_configured_for_missing_provider() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        let resolver = test_resolver(secrets, Arc::new(|_| None));
        let catalog = default_catalog();

        let err = resolver
            .resolve(
                ProviderId::anthropic(),
                CredentialUsage::ApiRequest,
                &catalog,
            )
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            ResolveError::NotConfigured(provider) if provider == ProviderId::anthropic()
        ));
    }

    #[tokio::test]
    async fn anthropic_api_credentials_use_x_api_key_header() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_token(&secrets, "ANTHROPIC_API_KEY", "anthropic-key")
            .await
            .unwrap();
        let resolver = test_resolver(secrets, Arc::new(|_| None));
        let catalog = default_catalog();

        let resolved = resolver
            .resolve(
                ProviderId::anthropic(),
                CredentialUsage::ApiRequest,
                &catalog,
            )
            .await
            .unwrap();
        let ResolvedCredential::Api(api) = resolved;

        assert_eq!(
            api.auth_header,
            Some(ApiKeyHeader::Custom {
                name:  "x-api-key".to_string(),
                value: "anthropic-key".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn custom_openai_compatible_resolves_with_catalog_base_url_from_vault() {
        let catalog = catalog_with(
            r#"
[providers.acme]
display_name = "Acme"
adapter = "openai_compatible"
agent_profile = "openai"
base_url = "https://default.example.com/v1"

[providers.acme.auth]
credentials = ["vault:acme"]

[models."compat-model"]
provider = "acme"
display_name = "Compat Model"
family = "openai"
default = true

[models."compat-model".limits]
context_window = 128000

[models."compat-model".features]
tools = true
vision = false
reasoning = false
"#,
        );
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_token(&secrets, "acme", "compat-key")
            .await
            .unwrap();
        let resolver = test_resolver(secrets, Arc::new(|_| None));
        let resolved = resolver
            .resolve(
                ProviderId::new("acme"),
                CredentialUsage::ApiRequest,
                &catalog,
            )
            .await
            .unwrap();

        let ResolvedCredential::Api(api) = resolved;
        assert_eq!(
            api.auth_header,
            Some(ApiKeyHeader::Bearer("compat-key".to_string()))
        );
        assert_eq!(
            api.base_url.as_deref(),
            Some("https://default.example.com/v1")
        );
    }

    #[tokio::test]
    async fn with_env_lookup_overrides_vault_settings() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_token(&secrets, "OPENAI_API_KEY", "vault-key")
            .await
            .unwrap();
        secrets
            .set(
                "OPENAI_ORG_ID",
                "vault-org",
                fabro_vault::SecretType::Token,
                None,
            )
            .await
            .unwrap();
        let resolver = test_resolver(
            secrets,
            Arc::new(|name| match name {
                "OPENAI_API_KEY" => Some("env-key".to_string()),
                "OPENAI_ORG_ID" => Some("env-org".to_string()),
                _ => None,
            }),
        );
        let catalog = default_catalog();

        let resolved = resolver
            .resolve(ProviderId::openai(), CredentialUsage::ApiRequest, &catalog)
            .await
            .unwrap();
        let ResolvedCredential::Api(api) = resolved;

        assert_eq!(api.org_id.as_deref(), Some("env-org"));
    }

    #[tokio::test]
    async fn configured_providers_returns_vault_backed_provider() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_token(&secrets, "OPENAI_API_KEY", "vault-key")
            .await
            .unwrap();
        let resolver = test_resolver(secrets, Arc::new(|_| None));
        let catalog = default_catalog();

        assert_eq!(resolver.configured_providers(&catalog).await, vec![
            ProviderId::openai()
        ]);
    }

    #[tokio::test]
    async fn resolve_uses_custom_vault_backed_provider() {
        let catalog = catalog_with(
            r#"
[providers.acme]
display_name = "Acme"
adapter = "openai_compatible"
agent_profile = "openai"
base_url = "https://api.acme.test/v1"

[providers.acme.auth]
credentials = ["vault:acme"]

[models."acme-large"]
provider = "acme"
display_name = "Acme Large"
family = "acme"
default = true

[models."acme-large".limits]
context_window = 128000

[models."acme-large".features]
tools = true
vision = false
reasoning = false
"#,
        );
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        secret_set_token(&secrets, "acme", "acme-key")
            .await
            .unwrap();
        let resolver = test_resolver(secrets, Arc::new(|_| None));

        let resolved = resolver
            .resolve(
                ProviderId::new("acme"),
                CredentialUsage::ApiRequest,
                &catalog,
            )
            .await
            .unwrap();

        let ResolvedCredential::Api(api) = resolved;
        assert_eq!(api.provider, ProviderId::new("acme"));
        assert_eq!(
            api.auth_header,
            Some(ApiKeyHeader::Bearer("acme-key".to_string()))
        );
        assert_eq!(api.base_url.as_deref(), Some("https://api.acme.test/v1"));
    }

    #[tokio::test]
    async fn configured_providers_returns_env_backed_provider() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        let resolver = test_resolver(
            secrets,
            Arc::new(|name| (name == "OPENAI_API_KEY").then(|| "env-key".to_string())),
        );
        let catalog = default_catalog();

        assert_eq!(resolver.configured_providers(&catalog).await, vec![
            ProviderId::openai()
        ]);
    }

    #[tokio::test]
    async fn resolve_refreshes_expired_oauth_credentials_and_persists_them() {
        let server = MockServer::start_async().await;
        let refresh_mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .form_urlencoded_tuple("grant_type", "refresh_token")
                    .form_urlencoded_tuple("client_id", "test-client")
                    .form_urlencoded_tuple("refresh_token", "refresh-token");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(
                        serde_json::json!({
                            "access_token": "new-access",
                            "refresh_token": "new-refresh",
                            "expires_in": 3600
                        })
                        .to_string(),
                    );
            })
            .await;

        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(
            SecretStore::load(dir.path().join("secrets.json"))
                .await
                .unwrap(),
        );
        secret_set_oauth(
            &secrets,
            crate::OPENAI_CODEX_VAULT_SECRET_NAME,
            &oauth_credential(
                server.url("/oauth/token"),
                Utc::now() - Duration::minutes(1),
            ),
        )
        .await
        .unwrap();
        let resolver =
            CredentialResolver::with_env_lookup(Arc::clone(&secrets), Arc::new(|_| None));
        let catalog = default_catalog();

        let resolved = resolver
            .resolve(ProviderId::openai(), CredentialUsage::ApiRequest, &catalog)
            .await
            .unwrap();
        let ResolvedCredential::Api(api) = resolved;

        assert_eq!(
            api.auth_header,
            Some(ApiKeyHeader::Bearer("new-access".to_string()))
        );
        assert!(api.codex_mode);

        let stored = secret_get_oauth(&secrets, crate::OPENAI_CODEX_VAULT_SECRET_NAME)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.tokens.access_token, "new-access");
        assert_eq!(stored.tokens.refresh_token.as_deref(), Some("new-refresh"));
        assert_eq!(stored.account_id.as_deref(), Some("acct_123"));
        refresh_mock.assert_async().await;
    }

    #[tokio::test]
    async fn resolve_returns_refresh_token_missing_when_expired_oauth_has_no_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap();
        let mut credential = oauth_credential(
            "https://auth.openai.com/oauth/token".to_string(),
            Utc::now() - Duration::minutes(1),
        );
        credential.tokens.refresh_token = None;
        secret_set_oauth(
            &secrets,
            crate::OPENAI_CODEX_VAULT_SECRET_NAME,
            &credential,
        )
        .await
        .unwrap();
        let resolver = test_resolver(secrets, Arc::new(|_| None));
        let catalog = default_catalog();

        let err = resolver
            .resolve(ProviderId::openai(), CredentialUsage::ApiRequest, &catalog)
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            ResolveError::RefreshTokenMissing(provider) if provider == ProviderId::openai()
        ));
    }

    #[test]
    fn auth_issue_message_formats_refresh_token_missing() {
        let message = auth_issue_message(
            &ProviderId::openai(),
            &ResolveError::RefreshTokenMissing(ProviderId::openai()),
        );

        assert_eq!(
            message,
            "openai requires re-authentication: refresh token missing"
        );
    }

    #[test]
    fn api_credential_debug_redacts_secret_material() {
        let credential = ApiCredential {
            provider:      ProviderId::openai(),
            auth_header:   Some(ApiKeyHeader::Bearer("sk-test".to_string())),
            extra_headers: HashMap::new(),
            base_url:      None,
            codex_mode:    false,
            org_id:        None,
            project_id:    None,
        };

        let debug = format!("{credential:?}");

        assert!(!debug.contains("sk-test"));
        assert!(debug.contains("REDACTED"));
    }
}
