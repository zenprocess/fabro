use std::collections::HashMap;
use std::path::Path;

use anyhow::Context as _;
use fabro_static::EnvVars;
use fabro_types::settings::ServerNamespace;
use fabro_vault::SecretStore;
use tracing::warn;

use crate::jwt_auth::{AuthMode, resolve_auth_mode_with_lookup, validate_auth_configuration};
use crate::migrations;
use crate::server_secrets::ServerSecrets;

pub(crate) fn resolve_startup(
    env_path: &Path,
    env_entries: HashMap<String, String>,
    settings: &ServerNamespace,
    secrets: &SecretStore,
) -> anyhow::Result<(AuthMode, ServerSecrets)> {
    let server_secrets = ServerSecrets::load(env_path, env_entries)?;
    let auth_secret_lookup = |name: &str| match name {
        EnvVars::GITHUB_APP_CLIENT_SECRET => secrets.try_get(name),
        _ => server_secrets.get(name),
    };
    let auth_mode = resolve_auth_mode_with_lookup(settings, auth_secret_lookup)?;
    Ok((auth_mode, server_secrets))
}

pub async fn load_startup_secrets(vault_path: impl AsRef<Path>) -> anyhow::Result<SecretStore> {
    let vault_path = vault_path.as_ref();
    match migrations::migrate_legacy_vault_file(vault_path) {
        Ok(report) if report.changed() => {
            let backup_path = report
                .backup_path
                .as_ref()
                .map_or_else(|| "<none>".to_string(), |path| path.display().to_string());
            warn!(
                migrated_entries = report.migrated_entries,
                skipped_entries = report.skipped_entries,
                backup_path = %backup_path,
                removal_deadline = migrations::LEGACY_VAULT_REMOVAL_DEADLINE,
                "Migrated legacy vault file"
            );
        }
        Ok(_) => {}
        Err(err) => {
            warn!(
                error = %err,
                removal_deadline = migrations::LEGACY_VAULT_REMOVAL_DEADLINE,
                "Legacy vault migration failed; continuing with normal vault load"
            );
        }
    }
    SecretStore::load(vault_path.to_path_buf())
        .await
        .with_context(|| format!("load vault {}", vault_path.display()))
}

pub(crate) async fn prepare_startup_secrets(
    vault_path: impl AsRef<Path>,
    server_env_path: impl AsRef<Path>,
    env_entries: &HashMap<String, String>,
) -> anyhow::Result<SecretStore> {
    let secrets = load_startup_secrets(vault_path).await?;
    let report = migrations::migrate_optional_server_env_secrets_to_vault(
        &secrets,
        server_env_path.as_ref(),
        env_entries,
    )
    .await
    .context("migrate optional server env secrets into vault")?;

    for warning in &report.warnings {
        warn!(
            warning = %warning,
            removal_deadline = migrations::OPTIONAL_SERVER_ENV_SECRETS_REMOVAL_DEADLINE,
            "Optional server env secrets migration warning"
        );
    }

    if report.changed() {
        let backup_path = report
            .backup_path
            .as_ref()
            .map_or_else(|| "<none>".to_string(), |path| path.display().to_string());
        warn!(
            migrated_secrets = report.migrated_secrets,
            removed_env_entries = report.removed_env_entries,
            preserved_env_entries = report.preserved_env_entries,
            backup_path = %backup_path,
            removal_deadline = migrations::OPTIONAL_SERVER_ENV_SECRETS_REMOVAL_DEADLINE,
            "Migrated optional server env secrets into vault"
        );
    }

    Ok(secrets)
}

pub fn validate_startup(
    env_path: &Path,
    env_entries: HashMap<String, String>,
    settings: &ServerNamespace,
    secrets: &SecretStore,
) -> anyhow::Result<()> {
    resolve_startup(env_path, env_entries, settings, secrets).map(|_| ())
}

pub fn validate_startup_configuration(settings: &ServerNamespace) -> anyhow::Result<()> {
    validate_auth_configuration(settings)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use fabro_config::{ServerSettingsBuilder, envfile};
    use fabro_static::EnvVars;
    use fabro_types::settings::ServerNamespace;
    use fabro_vault::{SecretStore, SecretType};

    use super::{prepare_startup_secrets, validate_startup};

    fn resolved_settings(auth_methods: &[&str]) -> ServerNamespace {
        ServerSettingsBuilder::from_toml(&format!(
            r#"
_version = 1

[server.auth]
methods = [{}]

[server.auth.github]
allowed_usernames = ["octocat"]

[server.integrations.github]
client_id = "Iv1.test"
"#,
            auth_methods
                .iter()
                .map(|method| format!("\"{method}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .unwrap()
        .server
    }

    async fn empty_secrets(dir: &tempfile::TempDir) -> SecretStore {
        SecretStore::load(dir.path().join("secrets.json"))
            .await
            .unwrap()
    }

    fn env_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("server.env")
    }

    fn vault_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("secrets.json")
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "test helper scans a temporary directory after startup migration completes"
    )]
    fn migration_backups(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.contains("optional-server-env-secrets-to-vault-migration")
                    })
            })
            .collect()
    }

    #[tokio::test]
    async fn validate_startup_accepts_configured_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = empty_secrets(&dir).await;
        let env = HashMap::from([
            (
                EnvVars::SESSION_SECRET.to_string(),
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            ),
            (
                EnvVars::FABRO_DEV_TOKEN.to_string(),
                "fabro_dev_abababababababababababababababababababababababababababababababab"
                    .to_string(),
            ),
        ]);
        let settings = resolved_settings(&["dev-token"]);

        assert!(
            validate_startup(
                dir.path().join("server.env").as_path(),
                env,
                &settings,
                &secrets,
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn validate_startup_rejects_missing_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let settings = resolved_settings(&["dev-token"]);
        let secrets = empty_secrets(&dir).await;

        assert!(
            validate_startup(
                dir.path().join("server.env").as_path(),
                HashMap::new(),
                &settings,
                &secrets,
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn validate_startup_requires_github_client_secret_from_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let env = HashMap::from([
            (
                EnvVars::SESSION_SECRET.to_string(),
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            ),
            (
                EnvVars::GITHUB_APP_CLIENT_SECRET.to_string(),
                "server-env-client-secret".to_string(),
            ),
        ]);
        let settings = resolved_settings(&["github"]);
        let secrets = empty_secrets(&dir).await;

        let err = validate_startup(
            dir.path().join("server.env").as_path(),
            env,
            &settings,
            &secrets,
        )
        .expect_err("github client secret in server.env should not satisfy startup");

        assert!(err.to_string().contains("GITHUB_APP_CLIENT_SECRET"));
    }

    #[tokio::test]
    async fn validate_startup_accepts_github_client_secret_from_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let env = HashMap::from([(
            EnvVars::SESSION_SECRET.to_string(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
        )]);
        let settings = resolved_settings(&["github"]);
        let secrets = empty_secrets(&dir).await;
        secrets
            .set(
                EnvVars::GITHUB_APP_CLIENT_SECRET,
                "vault-client-secret",
                SecretType::Token,
                None,
            )
            .await
            .unwrap();

        validate_startup(
            dir.path().join("server.env").as_path(),
            env,
            &settings,
            &secrets,
        )
        .expect("github client secret in vault should satisfy startup");
    }

    #[tokio::test]
    async fn prepare_startup_secrets_migrates_server_env_optional_secrets_to_store() {
        let dir = tempfile::tempdir().unwrap();
        let server_env_path = env_path(&dir);
        envfile::write_env_file(
            &server_env_path,
            &HashMap::from([
                (
                    EnvVars::SESSION_SECRET.to_string(),
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                ),
                (
                    EnvVars::GITHUB_APP_CLIENT_SECRET.to_string(),
                    "legacy-client-secret".to_string(),
                ),
                (
                    EnvVars::GITHUB_APP_PRIVATE_KEY.to_string(),
                    "legacy-private-key".to_string(),
                ),
                (EnvVars::OPENAI_API_KEY.to_string(), "sk-legacy".to_string()),
            ]),
        )
        .unwrap();

        let secrets = prepare_startup_secrets(vault_path(&dir), &server_env_path, &HashMap::new())
            .await
            .expect("legacy optional secrets should migrate");

        assert_eq!(
            secrets
                .get(EnvVars::GITHUB_APP_CLIENT_SECRET)
                .await
                .as_deref(),
            Some("legacy-client-secret")
        );
        assert_eq!(
            secrets
                .get_entry(EnvVars::GITHUB_APP_CLIENT_SECRET)
                .await
                .unwrap()
                .secret_type,
            SecretType::Token
        );
        assert_eq!(
            secrets
                .get(EnvVars::GITHUB_APP_PRIVATE_KEY)
                .await
                .as_deref(),
            Some("legacy-private-key")
        );
        assert_eq!(
            secrets
                .get_entry(EnvVars::GITHUB_APP_PRIVATE_KEY)
                .await
                .unwrap()
                .secret_type,
            SecretType::File
        );
        assert_eq!(
            secrets.get(EnvVars::OPENAI_API_KEY).await.as_deref(),
            Some("sk-legacy")
        );

        let server_env = envfile::read_env_file(&server_env_path).unwrap();
        assert!(server_env.contains_key(EnvVars::SESSION_SECRET));
        assert!(!server_env.contains_key(EnvVars::GITHUB_APP_CLIENT_SECRET));
        assert!(!server_env.contains_key(EnvVars::GITHUB_APP_PRIVATE_KEY));
        assert!(!server_env.contains_key(EnvVars::OPENAI_API_KEY));
        assert_eq!(migration_backups(dir.path()).len(), 1);
    }

    #[tokio::test]
    async fn prepare_startup_secrets_prefers_process_env_and_preserves_conflicting_server_env() {
        let dir = tempfile::tempdir().unwrap();
        let server_env_path = env_path(&dir);
        envfile::write_env_file(
            &server_env_path,
            &HashMap::from([(
                EnvVars::GITHUB_APP_CLIENT_SECRET.to_string(),
                "file-client-secret".to_string(),
            )]),
        )
        .unwrap();
        let env_entries = HashMap::from([(
            EnvVars::GITHUB_APP_CLIENT_SECRET.to_string(),
            "process-client-secret".to_string(),
        )]);

        let secrets = prepare_startup_secrets(vault_path(&dir), &server_env_path, &env_entries)
            .await
            .expect("process env secret should migrate");

        assert_eq!(
            secrets
                .get(EnvVars::GITHUB_APP_CLIENT_SECRET)
                .await
                .as_deref(),
            Some("process-client-secret")
        );
        let server_env = envfile::read_env_file(&server_env_path).unwrap();
        assert_eq!(
            server_env
                .get(EnvVars::GITHUB_APP_CLIENT_SECRET)
                .map(String::as_str),
            Some("file-client-secret")
        );
        assert!(migration_backups(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn prepare_startup_secrets_keeps_existing_secret_and_removes_matching_server_env() {
        let dir = tempfile::tempdir().unwrap();
        let server_env_path = env_path(&dir);
        envfile::write_env_file(
            &server_env_path,
            &HashMap::from([(
                EnvVars::GITHUB_APP_CLIENT_SECRET.to_string(),
                "vault-client-secret".to_string(),
            )]),
        )
        .unwrap();
        let secrets = SecretStore::load(vault_path(&dir)).await.unwrap();
        secrets
            .set(
                EnvVars::GITHUB_APP_CLIENT_SECRET,
                "vault-client-secret",
                SecretType::Token,
                None,
            )
            .await
            .unwrap();

        let secrets = prepare_startup_secrets(vault_path(&dir), &server_env_path, &HashMap::new())
            .await
            .expect("redundant server env secret should be cleaned up");

        assert_eq!(
            secrets
                .get(EnvVars::GITHUB_APP_CLIENT_SECRET)
                .await
                .as_deref(),
            Some("vault-client-secret")
        );
        let server_env = envfile::read_env_file(&server_env_path).unwrap();
        assert!(!server_env.contains_key(EnvVars::GITHUB_APP_CLIENT_SECRET));
        assert_eq!(migration_backups(dir.path()).len(), 1);
    }

    #[tokio::test]
    async fn prepare_startup_secrets_migrated_github_client_secret_satisfies_startup() {
        let dir = tempfile::tempdir().unwrap();
        let server_env_path = env_path(&dir);
        envfile::write_env_file(
            &server_env_path,
            &HashMap::from([
                (
                    EnvVars::SESSION_SECRET.to_string(),
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                ),
                (
                    EnvVars::GITHUB_APP_CLIENT_SECRET.to_string(),
                    "legacy-client-secret".to_string(),
                ),
            ]),
        )
        .unwrap();
        let settings = resolved_settings(&["github"]);

        let secrets = prepare_startup_secrets(vault_path(&dir), &server_env_path, &HashMap::new())
            .await
            .expect("legacy github client secret should migrate");

        validate_startup(&server_env_path, HashMap::new(), &settings, &secrets)
            .expect("migrated github client secret should satisfy startup");
    }
}
