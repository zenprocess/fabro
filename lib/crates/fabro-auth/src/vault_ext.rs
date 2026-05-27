use fabro_types::SecretMetadata;
use fabro_vault::{Error as SecretStoreError, SecretStore, SecretType};

use crate::credential::OAuthCredential;

#[derive(Debug, thiserror::Error)]
pub enum SecretLookupError {
    #[error("secret entry '{name}' has schema {actual:?}, expected {expected:?}")]
    SchemaMismatch {
        name:     String,
        expected: SecretType,
        actual:   SecretType,
    },
    #[error("secret entry '{name}' is not valid {expected:?} JSON: {source}")]
    DecodeFailed {
        name:     String,
        expected: SecretType,
        #[source]
        source:   serde_json::Error,
    },
}

pub async fn secret_get_token(
    secrets: &SecretStore,
    name: &str,
) -> Result<Option<String>, SecretLookupError> {
    let Some(entry) = secrets.get_entry(name).await else {
        return Ok(None);
    };
    if entry.secret_type != SecretType::Token {
        return Err(SecretLookupError::SchemaMismatch {
            name:     name.to_string(),
            expected: SecretType::Token,
            actual:   entry.secret_type,
        });
    }
    Ok(Some(entry.value.clone()))
}

pub async fn secret_get_oauth(
    secrets: &SecretStore,
    name: &str,
) -> Result<Option<OAuthCredential>, SecretLookupError> {
    let Some(entry) = secrets.get_entry(name).await else {
        return Ok(None);
    };
    if entry.secret_type != SecretType::Oauth {
        return Err(SecretLookupError::SchemaMismatch {
            name:     name.to_string(),
            expected: SecretType::Oauth,
            actual:   entry.secret_type,
        });
    }
    serde_json::from_str(&entry.value)
        .map(Some)
        .map_err(|source| SecretLookupError::DecodeFailed {
            name: name.to_string(),
            expected: SecretType::Oauth,
            source,
        })
}

pub async fn secret_set_token(
    secrets: &SecretStore,
    name: &str,
    value: &str,
) -> Result<SecretMetadata, SecretStoreError> {
    secrets.set(name, value, SecretType::Token, None).await
}

pub async fn secret_set_oauth(
    secrets: &SecretStore,
    name: &str,
    credential: &OAuthCredential,
) -> Result<SecretMetadata, SecretStoreError> {
    let json = serde_json::to_string(credential)?;
    secrets.set(name, &json, SecretType::Oauth, None).await
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::*;
    use crate::credential::{OAuthConfig, OAuthTokens};

    async fn temp_secrets() -> SecretStore {
        let dir = tempfile::tempdir().unwrap();
        SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap()
    }

    fn fixture() -> OAuthCredential {
        OAuthCredential {
            tokens:     OAuthTokens {
                access_token:  "access".to_string(),
                refresh_token: Some("refresh".to_string()),
                expires_at:    Utc::now() + Duration::hours(1),
            },
            config:     OAuthConfig {
                auth_url:     "https://auth.openai.com".to_string(),
                token_url:    "https://auth.openai.com/oauth/token".to_string(),
                client_id:    "client".to_string(),
                scopes:       vec!["openid".to_string()],
                redirect_uri: None,
                use_pkce:     true,
            },
            account_id: None,
        }
    }

    #[tokio::test]
    async fn secret_get_token_returns_none_when_absent() {
        let secrets = temp_secrets().await;
        assert!(
            secret_get_token(&secrets, "ANTHROPIC_API_KEY")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn secret_get_token_returns_value_when_present() {
        let secrets = temp_secrets().await;
        secret_set_token(&secrets, "ANTHROPIC_API_KEY", "sk-test")
            .await
            .unwrap();
        assert_eq!(
            secret_get_token(&secrets, "ANTHROPIC_API_KEY")
                .await
                .unwrap()
                .as_deref(),
            Some("sk-test"),
        );
    }

    #[tokio::test]
    async fn secret_get_token_errors_on_oauth_entry() {
        let secrets = temp_secrets().await;
        secret_set_oauth(
            &secrets,
            crate::OPENAI_CODEX_VAULT_SECRET_NAME,
            &fixture(),
        )
        .await
        .unwrap();
        let err = secret_get_token(&secrets, crate::OPENAI_CODEX_VAULT_SECRET_NAME)
            .await
            .unwrap_err();
        assert!(matches!(err, SecretLookupError::SchemaMismatch { .. }));
    }

    #[tokio::test]
    async fn secret_get_oauth_round_trips() {
        let secrets = temp_secrets().await;
        let credential = fixture();
        secret_set_oauth(
            &secrets,
            crate::OPENAI_CODEX_VAULT_SECRET_NAME,
            &credential,
        )
        .await
        .unwrap();
        assert_eq!(
            secret_get_oauth(&secrets, crate::OPENAI_CODEX_VAULT_SECRET_NAME)
                .await
                .unwrap()
                .unwrap(),
            credential,
        );
    }
}
