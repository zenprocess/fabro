use std::collections::BTreeSet;
use std::sync::Arc;

use fabro_model::catalog::{CredentialRef, HeaderValueRef, LlmCatalogSettings};
use fabro_model::{Catalog, ProviderId};
use fabro_static::EnvVars;
use fabro_types::settings::InterpString;
use fabro_types::settings::run::{EnvironmentProvider, McpTransport, RunMode, RunNamespace};
use fabro_types::settings::server::{GithubIntegrationSettings, GithubIntegrationStrategy};
use fabro_types::{
    Graph, ServerSettings, WorkerBootstrapGithubIntegration, WorkerBootstrapResponse,
    WorkerBootstrapSecret, is_llm_handler_type,
};
use fabro_vault::Vault;
use fabro_workflow::handler::llm::routing;
use fabro_workflow::operations;
use serde::Serialize;
use toml::ser;

use super::super::{
    ApiError, AppState, IntoResponse, Json, RequireWorkerRunScoped, Response, Router, State,
    StatusCode, get, header,
};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route(
        "/runs/{id}/worker/bootstrap",
        get(retrieve_worker_bootstrap),
    )
}

async fn retrieve_worker_bootstrap(
    RequireWorkerRunScoped(id): RequireWorkerRunScoped,
    State(state): State<Arc<AppState>>,
) -> Response {
    let cached = match state.store_ref().get_cached_run(&id).await {
        Ok(Some(cached)) => cached,
        Ok(None) => return ApiError::not_found("Run not found.").into_response(),
        Err(err) => {
            return ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response();
        }
    };

    let server_settings = state.server_settings();
    let github = github_bootstrap_metadata(&server_settings.server.integrations.github);
    let config_toml =
        match worker_bootstrap_config_toml(state.llm_catalog_settings().as_ref(), &github) {
            Ok(config_toml) => config_toml,
            Err(err) => {
                return ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response();
            }
        };

    let run_spec = &cached.projection.spec;
    let catalog = state.catalog();
    let configured_providers =
        operations::configured_providers_for_start(Some(&state.vault), Arc::clone(&catalog))
            .await
            .into_iter()
            .collect::<BTreeSet<_>>();
    let vault = state.vault.read().await;
    let selector = WorkerBootstrapSecretSelector {
        repo_origin_url:      run_spec.repo_origin_url(),
        run_settings:         &run_spec.settings.run,
        accepted_graph:       run_spec.graph(),
        catalog:              catalog.as_ref(),
        configured_providers: &configured_providers,
        server_settings:      server_settings.as_ref(),
        server_vault:         &vault,
    };
    let secrets = selector
        .required_secret_names()
        .into_iter()
        .filter_map(|name| secret_response_for_name(&vault, &name))
        .collect();

    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(WorkerBootstrapResponse {
            config_toml,
            secrets,
            github,
        }),
    )
        .into_response()
}

#[derive(Serialize)]
struct WorkerBootstrapConfig<'a> {
    #[serde(rename = "_version")]
    version: u8,
    llm:     &'a LlmCatalogSettings,
    server:  WorkerBootstrapServerConfig,
}

#[derive(Serialize)]
struct WorkerBootstrapServerConfig {
    integrations: WorkerBootstrapIntegrationsConfig,
}

#[derive(Serialize)]
struct WorkerBootstrapIntegrationsConfig {
    github: WorkerBootstrapGithubIntegration,
}

fn worker_bootstrap_config_toml(
    llm: &LlmCatalogSettings,
    github: &WorkerBootstrapGithubIntegration,
) -> Result<String, ser::Error> {
    toml::to_string(&WorkerBootstrapConfig {
        version: 1,
        llm,
        server: WorkerBootstrapServerConfig {
            integrations: WorkerBootstrapIntegrationsConfig {
                github: github.clone(),
            },
        },
    })
}

fn github_bootstrap_metadata(
    settings: &GithubIntegrationSettings,
) -> WorkerBootstrapGithubIntegration {
    WorkerBootstrapGithubIntegration {
        enabled:  settings.enabled,
        strategy: settings.strategy,
        app_id:   settings.app_id.as_ref().map(InterpString::as_source),
        slug:     settings.slug.as_ref().map(InterpString::as_source),
    }
}

fn secret_response_for_name(vault: &Vault, name: &str) -> Option<WorkerBootstrapSecret> {
    let entry = vault.get_entry(name)?;
    Some(WorkerBootstrapSecret {
        name:        name.to_string(),
        value:       entry.value.clone(),
        secret_type: entry.secret_type,
        description: entry.description.clone(),
    })
}

struct WorkerBootstrapSecretSelector<'a> {
    repo_origin_url:      Option<&'a str>,
    run_settings:         &'a RunNamespace,
    accepted_graph:       &'a Graph,
    catalog:              &'a Catalog,
    configured_providers: &'a BTreeSet<ProviderId>,
    server_settings:      &'a ServerSettings,
    server_vault:         &'a Vault,
}

impl WorkerBootstrapSecretSelector<'_> {
    fn required_secret_names(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        self.collect_llm_provider_secrets(&mut names);
        self.collect_mcp_header_secrets(&mut names);
        self.collect_github_secrets(&mut names);
        self.collect_sandbox_provider_secrets(&mut names);
        names.retain(|name| !fabro_static::is_bootstrap_secret(name));
        names
    }

    fn collect_llm_provider_secrets(&self, names: &mut BTreeSet<String>) {
        for provider_id in self.reachable_llm_provider_ids() {
            let Some(provider) = self.catalog.provider(&provider_id) else {
                continue;
            };
            if let Some(auth) = &provider.auth {
                for credential in &auth.credentials {
                    if let CredentialRef::Vault(name) = credential {
                        names.insert(name.clone());
                    }
                }
            }
            for header in provider.extra_headers.values() {
                if let HeaderValueRef::Vault(name) = header {
                    names.insert(name.clone());
                }
            }
        }
    }

    fn reachable_llm_provider_ids(&self) -> BTreeSet<ProviderId> {
        let configured = self
            .configured_providers
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let Ok(start) = operations::resolve_start_llm(self.catalog, &configured, self.run_settings)
        else {
            return BTreeSet::new();
        };

        let mut provider_ids = BTreeSet::new();
        provider_ids.insert(start.provider_id.clone());
        for fallback in &start.fallback_chain {
            provider_ids.insert(ProviderId::from(fallback.provider.as_str()));
        }
        for node in self.accepted_graph.nodes.values() {
            if !is_llm_handler_type(node.handler_type()) {
                continue;
            }
            if let Ok(context) = routing::resolve_node_provider_context(
                self.catalog,
                &start.provider_id,
                &start.model,
                node,
            ) {
                provider_ids.insert(context.provider_id);
            }
        }
        provider_ids
    }

    fn collect_mcp_header_secrets(&self, names: &mut BTreeSet<String>) {
        for mcp in self.run_settings.agent.mcps.values() {
            let McpTransport::Http { headers, .. } = &mcp.transport else {
                continue;
            };
            for value in headers.values() {
                let interpolated = InterpString::parse(value);
                for name in interpolated.env_var_names() {
                    if self.server_vault.get_entry(name).is_some() {
                        names.insert(name.to_string());
                    }
                }
            }
        }
    }

    fn collect_github_secrets(&self, names: &mut BTreeSet<String>) {
        if !self.github_credentials_needed() {
            return;
        }
        match self.server_settings.server.integrations.github.strategy {
            GithubIntegrationStrategy::Token => {
                names.insert(EnvVars::GITHUB_TOKEN.to_string());
            }
            GithubIntegrationStrategy::App => {
                names.insert(EnvVars::GITHUB_APP_PRIVATE_KEY.to_string());
            }
        }
    }

    fn github_credentials_needed(&self) -> bool {
        if self.run_settings.integrations.github.is_token_requested() {
            return true;
        }

        if self.run_settings.execution.mode == RunMode::DryRun {
            return false;
        }

        let clone_can_use_github_credentials =
            self.run_settings.environment.provider.is_clone_based()
                && self
                    .repo_origin_url
                    .is_some_and(|origin| !origin.trim().is_empty());
        let pull_request_can_use_github_credentials = self.run_settings.pull_request.is_some();
        clone_can_use_github_credentials || pull_request_can_use_github_credentials
    }

    fn collect_sandbox_provider_secrets(&self, names: &mut BTreeSet<String>) {
        if self.run_settings.environment.provider == EnvironmentProvider::Daytona {
            names.insert(EnvVars::DAYTONA_API_KEY.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use fabro_types::SecretType;
    use fabro_types::settings::run::RunNamespace;
    use ulid::Ulid;

    use super::*;

    #[test]
    fn worker_bootstrap_selector_includes_daytona_secret_only_for_daytona_runs() {
        let vault_path =
            std::env::temp_dir().join(format!("fabro-worker-bootstrap-{}.json", Ulid::new()));
        let mut vault = Vault::load(vault_path).expect("test vault should load");
        vault
            .set(
                EnvVars::DAYTONA_API_KEY,
                "dtn-test",
                SecretType::Token,
                None,
            )
            .expect("test vault entry should persist");
        let catalog = Catalog::from_builtin().expect("test catalog should build");
        let configured_providers = BTreeSet::new();
        let graph = Graph::new("test");
        let server_settings = fabro_config::ServerSettingsBuilder::from_toml(
            r#"
_version = 1

[server.auth]
methods = ["dev-token"]
"#,
        )
        .expect("server settings should resolve");

        let local_settings = RunNamespace::default();
        let local_selector = WorkerBootstrapSecretSelector {
            repo_origin_url:      None,
            run_settings:         &local_settings,
            accepted_graph:       &graph,
            catalog:              &catalog,
            configured_providers: &configured_providers,
            server_settings:      &server_settings,
            server_vault:         &vault,
        };
        assert!(
            !local_selector
                .required_secret_names()
                .contains(EnvVars::DAYTONA_API_KEY)
        );

        let mut daytona_settings = RunNamespace::default();
        daytona_settings.environment.provider = EnvironmentProvider::Daytona;
        let daytona_selector = WorkerBootstrapSecretSelector {
            repo_origin_url:      None,
            run_settings:         &daytona_settings,
            accepted_graph:       &graph,
            catalog:              &catalog,
            configured_providers: &configured_providers,
            server_settings:      &server_settings,
            server_vault:         &vault,
        };
        assert!(
            daytona_selector
                .required_secret_names()
                .contains(EnvVars::DAYTONA_API_KEY)
        );
    }
}
