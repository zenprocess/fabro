use async_trait::async_trait;
use fabro_types::{SandboxInfo, SandboxProviderKind};

use super::{SandboxCreateSpec, SandboxProvider};
use crate::forkd::{ForkdConfig, ForkdSandbox};
use crate::details;

/// A [`SandboxProvider`] that creates Firecracker microVMs via a forkd
/// controller.  The controller URL and bearer token are resolved once at
/// construction time from `FORKD_URL` / `FORKD_TOKEN`; they never appear in
/// per-run specs.
#[derive(Clone)]
pub struct ForkdSandboxProvider {
    config: ForkdConfig,
}

impl ForkdSandboxProvider {
    /// Build a provider from an already-resolved [`ForkdConfig`].
    pub fn new(config: ForkdConfig) -> Self {
        Self { config }
    }

    /// Build a provider by reading `FORKD_URL` and `FORKD_TOKEN` from the
    /// process environment (the same resolution that
    /// [`crate::from_environment::forkd_config_from_environment`] performs for
    /// a full [`RunEnvironmentSettings`]).
    #[expect(
        clippy::disallowed_methods,
        reason = "ForkdSandboxProvider::from_env resolves server-level credentials from the process environment."
    )]
    pub fn from_env() -> Self {
        let forkd_url = std::env::var("FORKD_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8889".to_string());
        let forkd_token = std::env::var("FORKD_TOKEN")
            .unwrap_or_else(|_| "forkd-local-token".to_string());
        Self::new(ForkdConfig {
            forkd_url,
            forkd_token,
            settings: Default::default(),
        })
    }

    fn http_client(&self) -> crate::Result<reqwest::Client> {
        reqwest::Client::builder()
            .build()
            .map_err(|e| crate::Error::context("Failed to build HTTP client for forkd", e))
    }
}

#[async_trait]
impl SandboxProvider for ForkdSandboxProvider {
    fn kind(&self) -> SandboxProviderKind {
        SandboxProviderKind::Forkd
    }

    async fn list(&self) -> crate::Result<Vec<SandboxInfo>> {
        let client = self.http_client()?;
        let url = format!("{}/vms", self.config.forkd_url);
        let resp = client
            .get(&url)
            .bearer_auth(&self.config.forkd_token)
            .send()
            .await
            .map_err(|e| crate::Error::context("Failed to list forkd VMs", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::Error::message(format!(
                "forkd list VMs returned {status}: {body}"
            )));
        }

        let vms: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::Error::context("Failed to parse forkd VM list", e))?;

        // forkd 0.5.2 returns { "vms": [ { "name": "...", "status": "...", ... } ] }
        let arr = vms
            .get("vms")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let sandboxes = arr
            .into_iter()
            .filter_map(|vm| {
                let name = vm.get("name")?.as_str()?.to_string();
                Some(details::forkd::forkd_info_from_name(&name))
            })
            .collect();

        Ok(sandboxes)
    }

    async fn get(&self, id: &str) -> crate::Result<Option<SandboxInfo>> {
        let client = self.http_client()?;
        let url = format!("{}/vms/{}", self.config.forkd_url, id);
        let resp = client
            .get(&url)
            .bearer_auth(&self.config.forkd_token)
            .send()
            .await
            .map_err(|e| crate::Error::context(format!("Failed to get forkd VM '{id}'"), e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::Error::message(format!(
                "forkd get VM '{id}' returned {status}: {body}"
            )));
        }

        Ok(Some(details::forkd::forkd_info_from_name(id)))
    }

    async fn create(&self, spec: SandboxCreateSpec) -> crate::Result<SandboxInfo> {
        let SandboxCreateSpec::Forkd {
            config,
            run_id,
            clone_origin_url,
            clone_branch,
        } = spec
        else {
            return Err(crate::Error::message(
                "ForkdSandboxProvider requires a SandboxCreateSpec::Forkd variant",
            ));
        };

        let merged_config = ForkdConfig {
            forkd_url:   self.config.forkd_url.clone(),
            forkd_token: self.config.forkd_token.clone(),
            settings:    *config,
        };

        let sandbox = ForkdSandbox::new(merged_config, run_id, clone_origin_url, clone_branch);
        let name = sandbox.vm_name().to_string();
        Ok(details::forkd::forkd_info_from_name(&name))
    }

    async fn delete(&self, id: &str) -> crate::Result<()> {
        let client = self.http_client()?;
        let url = format!("{}/vms/{}", self.config.forkd_url, id);
        let resp = client
            .delete(&url)
            .bearer_auth(&self.config.forkd_token)
            .send()
            .await
            .map_err(|e| crate::Error::context(format!("Failed to delete forkd VM '{id}'"), e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            // Already gone — treat as success.
            return Ok(());
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::Error::message(format!(
                "forkd delete VM '{id}' returned {status}: {body}"
            )));
        }

        Ok(())
    }
}
