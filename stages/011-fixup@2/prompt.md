Goal: # Secrets Rationalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Fabro server secret resolution simple and predictable: bootstrap secrets come only from process env or `server.env`; optional integration secrets come only from the vault.

**Architecture:** Introduce a small shared registry that classifies well-known secret names as bootstrap or optional. Keep `ServerSecrets` focused on bootstrap startup/runtime requirements, add vault-only helpers for optional integrations, and wire install mode so it can write optional secrets before enabling features such as GitHub auth.

**Tech Stack:** Rust workspace crates (`fabro-static`, `fabro-auth`, `fabro-agent`, `fabro-server`, `fabro-cli`), existing vault storage, existing install persistence, existing docs under `docs/internal` and `docs/public`.

---

## Decisions Locked In

- `server.env` is only for bootstrap/runtime secrets that the server may need before the vault can be used.
- Process env is also bootstrap-only for server runtime. Optional server integrations do not read process env.
- Vault is the only server/runtime source for optional integration secrets.
- Optional secrets may be workflow-visible for now.
- No compatibility shims or fallback aliases are required. Remove server-runtime `GH_TOKEN` fallback and optional `server.env` fallbacks.
- Non-secret configuration, such as base URLs, org IDs, or catalog settings, remains configuration. This plan only governs secrets.
- CLI/library code may still use env-backed credential sources where explicitly selected outside server runtime. The server runtime must not use that path for optional integrations.

## Secret Model

Bootstrap secrets:

- `SESSION_SECRET`
- `FABRO_DEV_TOKEN`
- object-store/storage credentials resolved by current server object-store builders, including manual AWS credentials such as `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and `AWS_SESSION_TOKEN`

Optional vault secrets:

- LLM provider API keys and OAuth credential records
- `FABRO_SLACK_BOT_TOKEN`
- `FABRO_SLACK_APP_TOKEN`
- `DAYTONA_API_KEY`
- `BRAVE_SEARCH_API_KEY`
- `GITHUB_TOKEN`
- `GITHUB_APP_PRIVATE_KEY`
- `GITHUB_APP_CLIENT_SECRET`
- `GITHUB_APP_WEBHOOK_SECRET`

## Change Map

- Create `lib/crates/fabro-static/src/secret_registry.rs` for shared classification.
- Modify `lib/crates/fabro-static/src/lib.rs` to export the registry.
- Modify `lib/crates/fabro-server/src/server_secrets.rs` to make the bootstrap boundary explicit in tests and naming.
- Modify `lib/crates/fabro-server/src/server.rs`, `diagnostics.rs`, `run_files.rs`, `run_manifest.rs`, `web_auth.rs`, `github_webhooks.rs`, and server handlers under `server/handler/` to use vault-only optional lookup.
- Modify `lib/crates/fabro-server/src/startup.rs`, `serve.rs`, and `jwt_auth.rs` so GitHub auth validates `GITHUB_APP_CLIENT_SECRET` from vault while session/dev-token auth still validates bootstrap secrets.
- Modify `lib/crates/fabro-auth/src/vault_source.rs` to expose an explicit vault-only credential source constructor for server runtime.
- Modify `lib/crates/fabro-agent/src/config.rs` and `tools.rs` so `web_search` receives `BRAVE_SEARCH_API_KEY` explicitly instead of reading process env inside the tool.
- Modify install flows in `lib/crates/fabro-server/src/install.rs` and `lib/crates/fabro-cli/src/commands/install.rs` so GitHub App secrets are written to vault, not `server.env`.
- Modify `lib/crates/fabro-server/src/server/handler/secrets.rs` so bootstrap secrets cannot be written through the vault API.
- Update public and internal docs after behavior is implemented.

---

## Task 1: Add The Shared Secret Registry

**Files:**

- Create: `lib/crates/fabro-static/src/secret_registry.rs`
- Modify: `lib/crates/fabro-static/src/lib.rs`
- Test: unit tests in `secret_registry.rs`

- [ ] **Step 1: Define the registry types and classified names**

Create `secret_registry.rs` with this shape:

```rust
use crate::EnvVars;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretScope {
    Bootstrap,
    OptionalVault,
}

const BOOTSTRAP_SECRETS: &[&str] = &[
    EnvVars::SESSION_SECRET,
    EnvVars::FABRO_DEV_TOKEN,
    EnvVars::AWS_ACCESS_KEY_ID,
    EnvVars::AWS_SECRET_ACCESS_KEY,
    EnvVars::AWS_SESSION_TOKEN,
];

const OPTIONAL_VAULT_SECRETS: &[&str] = &[
    EnvVars::ANTHROPIC_API_KEY,
    EnvVars::BRAVE_SEARCH_API_KEY,
    EnvVars::FABRO_SLACK_APP_TOKEN,
    EnvVars::FABRO_SLACK_BOT_TOKEN,
    EnvVars::GEMINI_API_KEY,
    EnvVars::GITHUB_APP_CLIENT_SECRET,
    EnvVars::GITHUB_APP_PRIVATE_KEY,
    EnvVars::GITHUB_APP_WEBHOOK_SECRET,
    EnvVars::GITHUB_TOKEN,
    EnvVars::INCEPTION_API_KEY,
    EnvVars::KIMI_API_KEY,
    EnvVars::MINIMAX_API_KEY,
    EnvVars::OPENAI_API_KEY,
    EnvVars::ZAI_API_KEY,
    EnvVars::DAYTONA_API_KEY,
];

pub fn secret_scope(name: &str) -> Option<SecretScope> {
    if BOOTSTRAP_SECRETS.contains(&name) {
        Some(SecretScope::Bootstrap)
    } else if OPTIONAL_VAULT_SECRETS.contains(&name) {
        Some(SecretScope::OptionalVault)
    } else {
        None
    }
}

pub fn is_bootstrap_secret(name: &str) -> bool {
    secret_scope(name) == Some(SecretScope::Bootstrap)
}

pub fn is_optional_vault_secret(name: &str) -> bool {
    secret_scope(name) == Some(SecretScope::OptionalVault)
}
```

- [ ] **Step 2: Export the registry**

Modify `lib.rs`:

```rust
mod env_vars;
mod secret_registry;

pub use env_vars::EnvVars;
pub use secret_registry::{
    SecretScope, is_bootstrap_secret, is_optional_vault_secret, secret_scope,
};
```

- [ ] **Step 3: Add registry tests**

Add tests proving:

- `SESSION_SECRET` and `FABRO_DEV_TOKEN` are bootstrap.
- `GITHUB_APP_CLIENT_SECRET`, `GITHUB_APP_PRIVATE_KEY`, `GITHUB_TOKEN`, Slack tokens, Daytona, Brave, and common LLM API keys are optional vault secrets.
- `GH_TOKEN` is not classified.
- non-secret config names such as `GITHUB_BASE_URL`, `SLACK_BASE_URL`, and `DAYTONA_API_URL` are not classified.

- [ ] **Step 4: Run the focused tests**

Run:

```bash
cargo nextest run -p fabro-static
```

Expected: all `fabro-static` tests pass.

- [ ] **Step 5: Commit**

```bash
git add lib/crates/fabro-static/src/lib.rs lib/crates/fabro-static/src/secret_registry.rs
git commit -m "refactor: classify server secret scopes"
```

---

## Task 2: Enforce Bootstrap Boundaries In The Secret API

**Files:**

- Modify: `lib/crates/fabro-server/src/server/handler/secrets.rs`
- Test: existing server handler tests or new focused tests near secret handler coverage

- [ ] **Step 1: Reject bootstrap secrets in `create_secret`**

Before OAuth parsing or Daytona validation, reject `SecretScope::Bootstrap`:

```rust
if fabro_static::is_bootstrap_secret(&name) {
    return ApiError::bad_request(format!(
        "{name} is a bootstrap secret; configure it with process env or server.env"
    ))
    .into_response();
}
```

- [ ] **Step 2: Keep optional and unknown secret writes allowed**

Do not reject unknown names. Workflows may define arbitrary vault-visible secrets.

- [ ] **Step 3: Add API tests**

Add tests proving:

- `POST /secrets` with `SESSION_SECRET` returns `400`.
- `POST /secrets` with `FABRO_DEV_TOKEN` returns `400`.
- `POST /secrets` with `GITHUB_APP_CLIENT_SECRET` succeeds.
- `POST /secrets` with a custom name such as `CUSTOM_WORKFLOW_TOKEN` succeeds.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-server secret
```

Expected: secret handler tests pass.

- [ ] **Step 5: Commit**

```bash
git add lib/crates/fabro-server/src/server/handler/secrets.rs lib/crates/fabro-server
git commit -m "fix: reject bootstrap secrets in vault API"
```

---

## Task 3: Replace Server Optional Lookups With Vault-Only Lookups

**Files:**

- Modify: `lib/crates/fabro-server/src/server.rs`
- Modify: `lib/crates/fabro-server/src/diagnostics.rs`
- Modify: `lib/crates/fabro-server/src/run_files.rs`
- Modify: `lib/crates/fabro-server/src/run_manifest.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/sandbox.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/sessions.rs`
- Test: `lib/crates/fabro-server/src/server/tests.rs` and existing handler tests

- [ ] **Step 1: Add explicit helpers on `AppState`**

Replace `vault_or_env` and `vault_or_env_pub` with:

```rust
pub(crate) fn vault_secret(&self, name: &str) -> Option<String> {
    self.vault
        .try_read()
        .ok()
        .and_then(|vault| vault.get(name).map(str::to_string))
}

fn config_env_lookup(&self, name: &str) -> Option<String> {
    (self.env_lookup)(name)
}
```

Keep `server_secret` for bootstrap secrets only.

- [ ] **Step 2: Change Daytona secret reads**

Use `state.vault_secret(EnvVars::DAYTONA_API_KEY)` wherever a Daytona API key is required. Keep non-secret Daytona URL and organization settings on `env_lookup` or settings-derived configuration, not vault lookup.

- [ ] **Step 3: Change GitHub token reads**

In `github_credentials`, resolve token strategy only through:

```rust
let token = self
    .vault_secret(EnvVars::GITHUB_TOKEN)
    .as_deref()
    .map(str::trim)
    .filter(|token| !token.is_empty())
    .map(str::to_string);
```

Remove the `GH_TOKEN` fallback and update the error message to say:

```text
GITHUB_TOKEN not configured -- run fabro install or run fabro secret set GITHUB_TOKEN
```

- [ ] **Step 4: Change diagnostics and handlers**

Replace `state.vault_or_env(...)` and `state.vault_or_env_pub(...)` with `state.vault_secret(...)` for optional secret checks. Update diagnostics remediation text to use `fabro secret set`.

- [ ] **Step 5: Add regression tests**

Add tests proving:

- `DAYTONA_API_KEY` in process env but absent from vault is treated as missing by server diagnostics.
- `GITHUB_TOKEN` in process env but absent from vault is treated as missing by token strategy.
- `GH_TOKEN` in vault or process env is ignored by server runtime.
- `DAYTONA_API_KEY` in vault is accepted.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-server daytona
cargo nextest run -p fabro-server github
```

Expected: relevant server tests pass.

- [ ] **Step 7: Commit**

```bash
git add lib/crates/fabro-server
git commit -m "refactor: resolve optional server secrets from vault only"
```

---

## Task 4: Make Server LLM Credentials Vault-Only

**Files:**

- Modify: `lib/crates/fabro-auth/src/vault_source.rs`
- Modify: `lib/crates/fabro-server/src/server.rs`
- Test: `lib/crates/fabro-auth/src/vault_source.rs`, `lib/crates/fabro-server/src/server/tests.rs`

- [ ] **Step 1: Add an explicit vault-only constructor**

Add this constructor to `VaultCredentialSource`:

```rust
#[must_use]
pub fn vault_only(vault: Arc<AsyncRwLock<Vault>>) -> Self {
    Self::with_env_lookup(vault, |_| None)
}
```

- [ ] **Step 2: Use vault-only source in server app state**

In `build_app_state`, change the LLM source construction from `with_env_lookup(... env_lookup ...)` to:

```rust
let llm_source: Arc<dyn CredentialSource> =
    Arc::new(VaultCredentialSource::vault_only(Arc::clone(&vault)));
```

- [ ] **Step 3: Keep CLI/library env sources explicit**

Do not remove `EnvCredentialSource`. Do not change direct CLI/library flows that intentionally choose env-backed credentials outside server runtime.

- [ ] **Step 4: Add tests**

Add tests proving:

- `VaultCredentialSource::vault_only` does not resolve process env values.
- Server readiness sees a provider configured only when the matching vault secret exists.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-auth
cargo nextest run -p fabro-server llm
```

Expected: auth and server LLM tests pass.

- [ ] **Step 6: Commit**

```bash
git add lib/crates/fabro-auth/src/vault_source.rs lib/crates/fabro-server/src/server.rs lib/crates/fabro-server/src/server/tests.rs
git commit -m "fix: use vault-only llm credentials in server runtime"
```

---

## Task 5: Move GitHub App Runtime Secrets To Vault

**Files:**

- Modify: `lib/crates/fabro-server/src/server.rs`
- Modify: `lib/crates/fabro-server/src/web_auth.rs`
- Modify: `lib/crates/fabro-server/src/github_webhooks.rs`
- Modify: `lib/crates/fabro-server/src/diagnostics.rs`
- Modify: `lib/crates/fabro-server/src/startup.rs`
- Modify: `lib/crates/fabro-server/src/serve.rs`
- Modify: `lib/crates/fabro-server/src/jwt_auth.rs`
- Test: server startup, auth, webhook, diagnostics, worker command tests

- [ ] **Step 1: Read GitHub App private key from vault**

In `AppState::github_credentials`, replace:

```rust
let raw = self.server_secret(EnvVars::GITHUB_APP_PRIVATE_KEY);
```

with:

```rust
let raw = self.vault_secret(EnvVars::GITHUB_APP_PRIVATE_KEY);
```

- [ ] **Step 2: Read OAuth client secret from vault**

In the GitHub OAuth callback handler, replace `state.server_secret(EnvVars::GITHUB_APP_CLIENT_SECRET)` with `state.vault_secret(EnvVars::GITHUB_APP_CLIENT_SECRET)`.

- [ ] **Step 3: Read webhook secret from vault**

Replace webhook secret resolution from `server_secret(WEBHOOK_SECRET_ENV)` to `vault_secret(WEBHOOK_SECRET_ENV)`. Startup logging should report webhook presence based on the vault value after the vault is loaded.

- [ ] **Step 4: Validate GitHub auth after vault load**

Change startup validation so `SESSION_SECRET` and `FABRO_DEV_TOKEN` are read from `ServerSecrets`, while `GITHUB_APP_CLIENT_SECRET` is read from the vault.

Use a composite lookup for auth validation:

```rust
let auth_secret_lookup = |name: &str| match name {
    EnvVars::GITHUB_APP_CLIENT_SECRET => vault.get(name).map(str::to_string),
    _ => server_secrets.get(name),
};
```

Load the vault before calling GitHub-auth validation. Pass the loaded vault into `build_app_state` or factor vault loading so the server does not perform inconsistent duplicate loads.

- [ ] **Step 5: Update worker GitHub App key injection**

Where the worker command currently forwards `GITHUB_APP_PRIVATE_KEY` from `server_secret`, forward it from `vault_secret`. Keep the explicit worker injection because worker environments remain scrubbed by default.

- [ ] **Step 6: Update tests**

Add or change tests proving:

- GitHub auth enabled with `GITHUB_APP_CLIENT_SECRET` only in `server.env` fails startup.
- GitHub auth enabled with `GITHUB_APP_CLIENT_SECRET` in vault passes startup.
- OAuth callback reads the vault client secret.
- Webhook verification reads the vault webhook secret.
- Worker command forwards the GitHub App private key from vault.
- Diagnostics reports missing GitHub App secrets based on vault, not `server.env`.

- [ ] **Step 7: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-server github
cargo nextest run -p fabro-server worker_auth
cargo nextest run -p fabro-cli server_start
```

Expected: GitHub auth, webhook, worker, and startup tests pass.

- [ ] **Step 8: Commit**

```bash
git add lib/crates/fabro-server lib/crates/fabro-cli/tests
git commit -m "fix: store github app runtime secrets in vault"
```

---

## Task 6: Move Slack To Vault-Only Credentials

**Files:**

- Modify: `lib/crates/fabro-server/src/server.rs`
- Modify: `lib/crates/fabro-slack/src/config.rs` only if names or status messages need adjustment
- Test: Slack credential resolution and server app-state tests

- [ ] **Step 1: Resolve Slack tokens from vault**

In `build_app_state`, replace:

```rust
resolve_slack_credentials_status_with_lookup(|name| server_secrets.get(name))
```

with:

```rust
resolve_slack_credentials_status_with_lookup(|name| {
    vault.try_read().ok().and_then(|vault| vault.get(name).map(str::to_string))
})
```

If the implementation already holds a synchronous vault read guard in this block, reuse that guard rather than calling `try_read` repeatedly.

- [ ] **Step 2: Keep Slack base URL out of the secret model**

Leave `SLACK_BASE_URL` as a non-secret test/development override. Do not add it to the secret registry.

- [ ] **Step 3: Update Slack tests**

Add tests proving:

- Slack tokens in vault enable Slack service.
- Slack tokens in `server.env` but absent from vault do not enable Slack service.
- Missing vault tokens produce the existing disabled status with missing token names.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-slack
cargo nextest run -p fabro-server slack
```

Expected: Slack tests pass.

- [ ] **Step 5: Commit**

```bash
git add lib/crates/fabro-server/src/server.rs lib/crates/fabro-slack/src/config.rs
git commit -m "fix: resolve slack credentials from vault"
```

---

## Task 7: Make Brave Search Tool Use Server-Provided Secrets

**Files:**

- Modify: `lib/crates/fabro-agent/src/config.rs`
- Modify: `lib/crates/fabro-agent/src/tools.rs`
- Modify: server session/profile construction where `SessionOptions` are built
- Test: `lib/crates/fabro-agent/src/tools.rs`, server diagnostics/tool integration tests

- [ ] **Step 1: Add tool secret configuration**

Add to `config.rs`:

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolSecrets {
    pub brave_search_api_key: Option<String>,
}
```

Add this field to `SessionOptions`:

```rust
pub tool_secrets: ToolSecrets,
```

Default it to `ToolSecrets::default()` and include it in the debug impl without printing secret values. Use a boolean:

```rust
.field(
    "brave_search_configured",
    &self.tool_secrets.brave_search_api_key.is_some(),
)
```

- [ ] **Step 2: Pass the Brave key into tool registration**

Change `register_core_tools`:

```rust
registry.register(make_web_search_tool_with_api_key(
    config.tool_secrets.brave_search_api_key.clone(),
));
```

Remove the process-env-reading `make_web_search_tool` wrapper.

- [ ] **Step 3: Improve the missing-key message**

Change the error text from “environment variable is not set” to:

```text
BRAVE_SEARCH_API_KEY is not configured
```

- [ ] **Step 4: Populate `SessionOptions.tool_secrets` in server runtime**

Where the server constructs agent sessions, set:

```rust
tool_secrets: ToolSecrets {
    brave_search_api_key: state.vault_secret(EnvVars::BRAVE_SEARCH_API_KEY),
},
```

Keep direct CLI agent behavior explicit: if the CLI should support local env-backed Brave Search, set this field at the CLI boundary from `std::env::var(EnvVars::BRAVE_SEARCH_API_KEY).ok()`. Do not let the tool read process env internally.

- [ ] **Step 5: Update tests**

Add tests proving:

- `make_web_search_tool_with_api_key(None)` returns `BRAVE_SEARCH_API_KEY is not configured`.
- `register_core_tools` wires `SessionOptions.tool_secrets.brave_search_api_key` into the web search tool.
- Server diagnostics and server-created sessions both use the same vault-backed key.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-agent web_search
cargo nextest run -p fabro-server brave
```

Expected: Brave tests pass.

- [ ] **Step 7: Commit**

```bash
git add lib/crates/fabro-agent lib/crates/fabro-server
git commit -m "fix: pass brave search credentials through server configuration"
```

---

## Task 8: Update Install Mode Persistence

**Files:**

- Modify: `lib/crates/fabro-server/src/install.rs`
- Modify: `lib/crates/fabro-cli/src/commands/install.rs`
- Modify: install tests in `lib/crates/fabro-server/tests/it/api/install.rs` and `lib/crates/fabro-cli/tests/it/cmd/install.rs`

- [ ] **Step 1: Server install writes GitHub App secrets to vault**

In `post_install_finish`, replace GitHub App `server_env_writes.push(...)` calls with `vault_secrets.push(...)` calls:

```rust
vault_secrets.push(VaultSecretWrite {
    name: EnvVars::GITHUB_APP_PRIVATE_KEY.to_string(),
    value: BASE64_STANDARD.encode(github.pem.as_bytes()),
    secret_type: VaultSecretType::File,
    description: None,
});

vault_secrets.push(VaultSecretWrite {
    name: EnvVars::GITHUB_APP_CLIENT_SECRET.to_string(),
    value: github.client_secret,
    secret_type: VaultSecretType::Token,
    description: None,
});

if let Some(secret) = github.webhook_secret {
    vault_secrets.push(VaultSecretWrite {
        name: EnvVars::GITHUB_APP_WEBHOOK_SECRET.to_string(),
        value: secret,
        secret_type: VaultSecretType::Token,
        description: None,
    });
}
```

- [ ] **Step 2: Remove stale GitHub App keys from `server.env` during install persistence**

Add removals for:

- `GITHUB_APP_PRIVATE_KEY`
- `GITHUB_APP_CLIENT_SECRET`
- `GITHUB_APP_WEBHOOK_SECRET`

This keeps greenfield installs clean and removes stale optional secrets if a developer has run earlier local install attempts.

- [ ] **Step 3: Align CLI GitHub install helper**

Update CLI install persistence so switching to GitHub App writes app secrets to vault and removes the app keys from `server.env`. Switching to token continues to write `GITHUB_TOKEN` to vault and remove app vault secrets.

- [ ] **Step 4: Update install tests**

Add or change tests proving:

- Browser install with GitHub App writes private key, client secret, and webhook secret to vault.
- Browser install with GitHub App does not write those keys to `server.env`.
- CLI GitHub App install writes app secrets to vault.
- Switching strategies removes stale secrets from the other strategy.
- `SESSION_SECRET`, `FABRO_DEV_TOKEN`, and object-store credentials remain in `server.env`.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-server install
cargo nextest run -p fabro-cli install
```

Expected: install tests pass.

- [ ] **Step 6: Commit**

```bash
git add lib/crates/fabro-server/src/install.rs lib/crates/fabro-server/tests/it/api/install.rs lib/crates/fabro-cli/src/commands/install.rs lib/crates/fabro-cli/tests/it/cmd/install.rs
git commit -m "fix: persist optional install secrets in vault"
```

---

## Task 9: Update Strategy And Public Docs

**Files:**

- Modify: `docs/internal/server-secrets-strategy.md`
- Modify: `docs/public/administration/server-configuration.mdx`
- Modify: `docs/public/administration/self-host-docker.mdx`
- Modify: `docs/public/administration/deploy-railway.mdx`
- Modify: `docs/public/integrations/github.mdx`
- Modify: `docs/public/integrations/slack.mdx`
- Modify: `docs/public/integrations/brave-search.mdx`
- Modify: `docs/public/integrations/daytona.mdx`
- Modify: LLM provider docs that still say server runtime reads provider keys from process env

- [ ] **Step 1: Rewrite internal strategy**

Update `server-secrets-strategy.md` to state:

- `ServerSecrets` means bootstrap secrets only.
- Bootstrap source precedence is process env then `server.env`.
- Optional integration secrets are vault-only.
- Requiredness is independent from source: GitHub auth may require a vault secret at startup.
- Server worker env remains scrubbed and receives only explicit injected values.

- [ ] **Step 2: Update administration docs**

State that `server.env` is for:

- `SESSION_SECRET`
- `FABRO_DEV_TOKEN`
- storage/object-store bootstrap credentials

State that `server.env` is not used for:

- Slack
- Daytona
- Brave Search
- LLM provider keys
- GitHub token
- GitHub App private key/client secret/webhook secret

- [ ] **Step 3: Update integration docs**

For each optional integration, show `fabro secret set` as the server-runtime configuration path:

```bash
fabro secret set BRAVE_SEARCH_API_KEY <key>
fabro secret set DAYTONA_API_KEY <key>
fabro secret set FABRO_SLACK_BOT_TOKEN <xoxb-token>
fabro secret set FABRO_SLACK_APP_TOKEN <xapp-token>
fabro secret set GITHUB_TOKEN <token>
```

For GitHub App, document that install mode stores app secrets in vault.

- [ ] **Step 4: Remove stale `server.env` guidance**

Search:

```bash
rg -n "server.env|process env -> server.env|GITHUB_APP_CLIENT_SECRET|FABRO_SLACK" docs/public docs/internal
```

Each result should either describe bootstrap secrets or explicitly say the secret is vault-only.

- [ ] **Step 5: Commit**

```bash
git add docs/internal docs/public
git commit -m "docs: document vault-only optional secrets"
```

---

## Task 10: Final Verification And Cleanup

**Files:**

- Modify any touched tests or docs needed by final verification.

- [ ] **Step 1: Search for removed server-runtime patterns**

Run:

```bash
rg -n "vault_or_env|vault_or_env_pub|GH_TOKEN|GITHUB_APP_CLIENT_SECRET.*server_secret|FABRO_SLACK_.*server_secret|BRAVE_SEARCH_API_KEY.*std::env|DAYTONA_API_KEY.*process_env" lib/crates docs
```

Expected:

- no server-runtime `vault_or_env` helper remains;
- no server-runtime `GH_TOKEN` fallback remains;
- no optional secret docs claim `server.env` is a valid server-runtime source;
- live-test annotations and explicit CLI/library env usage may remain.

- [ ] **Step 2: Run focused crate tests**

Run:

```bash
cargo nextest run -p fabro-static
cargo nextest run -p fabro-auth
cargo nextest run -p fabro-agent web_search
cargo nextest run -p fabro-slack
cargo nextest run -p fabro-server
cargo nextest run -p fabro-cli install
```

Expected: all listed tests pass.

- [ ] **Step 3: Run workspace quality checks**

Run:

```bash
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

Expected: formatting and clippy pass.

- [ ] **Step 4: Run one end-to-end install smoke if credentials are available**

Run a non-live smoke that exercises install persistence and restart validation through existing install tests. If local live credentials are available, run the relevant ignored/live tests for GitHub App, Daytona, and Brave Search.

- [ ] **Step 5: Final commit**

If Task 10 required cleanup changes, commit them:

```bash
git add .
git commit -m "test: verify rationalized server secrets"
```

If Task 10 produced no file changes, do not create an empty commit.

## Acceptance Criteria

- `server.env` is no longer a provider for Slack, Daytona, Brave Search, LLM provider credentials, GitHub token, or GitHub App secrets.
- Process env is no longer a server-runtime provider for optional integration secrets.
- Install mode can enable GitHub auth in one pass by writing GitHub App secrets to vault before normal startup.
- Startup fails clearly when GitHub auth is enabled and `GITHUB_APP_CLIENT_SECRET` is missing from vault.
- `fabro secret set` rejects bootstrap secrets and accepts optional integration secrets.
- Brave Search diagnostics and the actual `web_search` tool agree on whether the key is configured.
- Worker subprocess env remains scrubbed, with only explicit internal injections preserved.


## Completed stages
- **toolchain**: succeeded
  - Script: `command -v cargo >/dev/null || { curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && sudo ln -sf $HOME/.cargo/bin/* /usr/local/bin/; }; cargo --version 2>&1`
  - Output:
    ```
    cargo 1.95.0 (f2d3ce0bd 2026-03-21)
    ```
- **preflight_compile**: succeeded
  - Script: `cargo check -q --workspace 2>&1`
  - Output: (empty)
- **preflight_lint**: succeeded
  - Script: `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings 2>&1`
  - Output: (empty)
- **implement**: succeeded
  - Model: gpt-5.5, 5.5m tokens in / 25.6k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-static/src/secret_registry.rs
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 154.4k tokens in / 47.8k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/install.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/cmd/worker_auth.rs, /home/daytona/workspace/fabro/lib/crates/fabro-cli/tests/it/support/auth_harness.rs, /home/daytona/workspace/fabro/lib/crates/fabro-install/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/install.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/test_support.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/helpers.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/openapi_conformance.rs, /home/daytona/workspace/fabro/lib/crates/fabro-static/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-static/src/secret_registry.rs
- **simplify_gpt**: succeeded
  - Model: gpt-5.5, 588.5k tokens in / 5.2k out
- **verify**: failed
  - Script: `git fetch origin main 2>&1 && git merge --no-edit --no-stat origin/main 2>&1 && cargo +nightly-2026-04-14 fmt --all 2>&1 && cargo dev docs refresh 2>&1 && cargo +nightly-2026-04-14 fmt --check --all 2>&1 && { command -v rg >/dev/null 2>&1 || { echo 'rg is required for verify'; exit 127; }; } && ! rg -n 'AuthMode::Disabled|RunAuthMethod|RunSubjectProvenance|\bActorRef\b|\bActorKind\b|AuthenticatedSubject|AuthenticatedService|AuthorizeRunScoped|AuthorizeRunBlob|AuthorizeStageArtifact|AuthorizeCommandLog|auth_method\s*==\s*"disabled"' lib/crates apps lib/packages docs/public/api-reference/fabro-api.yaml 2>&1 && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings 2>&1 && cargo nextest run --workspace --status-level slow --profile ci 2>&1 && cargo dev docs check 2>&1 && bun install --frozen-lockfile 2>&1 && (cd apps/fabro-web && bun run typecheck) 2>&1 && (cd apps/fabro-web && bun run test) 2>&1 && (cd lib/packages/fabro-api-client && bun run typecheck) 2>&1 && cargo dev build -- -p fabro-cli --release 2>&1`
  - Output:
    ```
    (2037 lines omitted)
    react-test-renderer is deprecated. See https://react.dev/warnings/react-test-renderer
    The current testing environment is not configured to support act(...)
    The current testing environment is not configured to support act(...)
    (pass) VncPanel render > renders an iframe with the signed URL on success [0.76ms]
    react-test-renderer is deprecated. See https://react.dev/warnings/react-test-renderer
    The current testing environment is not configured to support act(...)
    The current testing environment is not configured to support act(...)
    (pass) VncPanel render > renders an actionable error state for 409 startup failures [0.90ms]
    react-test-renderer is deprecated. See https://react.dev/warnings/react-test-renderer
    The current testing environment is not configured to support act(...)
    The current testing environment is not configured to support act(...)
    (pass) VncPanel render > reconnect button refetches the signed URL [0.77ms]
    
    5 tests failed:
    (fail) StageInsightsSidebar > renders permission badge for read-only [1.03ms]
    (fail) StageInsightsSidebar > renders permission badge for full access [1.03ms]
    (fail) StageInsightsSidebar > renders projected agent tool names, descriptions, categories, and invoked state [1.21ms]
    (fail) StageInsightsSidebar > legacy stages without agent tools keep permission fallback only [0.78ms]
    (fail) StageInsightsSidebar > renders empty-friendly content when stage projection is missing [0.78ms]
    
     484 pass
     5 fail
     1189 expect() calls
    Ran 489 tests across 59 files. [10.06s]
    error: script "test" exited with code 1
    ```
- **fixup**: succeeded
  - Model: claude-opus-4-7, 29.5k tokens in / 1.7k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/api/runs.rs
- **verify**: failed
  - Script: `git fetch origin main 2>&1 && git merge --no-edit --no-stat origin/main 2>&1 && cargo +nightly-2026-04-14 fmt --all 2>&1 && cargo dev docs refresh 2>&1 && cargo +nightly-2026-04-14 fmt --check --all 2>&1 && { command -v rg >/dev/null 2>&1 || { echo 'rg is required for verify'; exit 127; }; } && ! rg -n 'AuthMode::Disabled|RunAuthMethod|RunSubjectProvenance|\bActorRef\b|\bActorKind\b|AuthenticatedSubject|AuthenticatedService|AuthorizeRunScoped|AuthorizeRunBlob|AuthorizeStageArtifact|AuthorizeCommandLog|auth_method\s*==\s*"disabled"' lib/crates apps lib/packages docs/public/api-reference/fabro-api.yaml 2>&1 && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings 2>&1 && cargo nextest run --workspace --status-level slow --profile ci 2>&1 && cargo dev docs check 2>&1 && bun install --frozen-lockfile 2>&1 && (cd apps/fabro-web && bun run typecheck) 2>&1 && (cd apps/fabro-web && bun run test) 2>&1 && (cd lib/packages/fabro-api-client && bun run typecheck) 2>&1 && cargo dev build -- -p fabro-cli --release 2>&1`
  - Output:
    ```
    (2037 lines omitted)
    react-test-renderer is deprecated. See https://react.dev/warnings/react-test-renderer
    The current testing environment is not configured to support act(...)
    The current testing environment is not configured to support act(...)
    (pass) VncPanel render > renders an iframe with the signed URL on success [0.76ms]
    react-test-renderer is deprecated. See https://react.dev/warnings/react-test-renderer
    The current testing environment is not configured to support act(...)
    The current testing environment is not configured to support act(...)
    (pass) VncPanel render > renders an actionable error state for 409 startup failures [0.90ms]
    react-test-renderer is deprecated. See https://react.dev/warnings/react-test-renderer
    The current testing environment is not configured to support act(...)
    The current testing environment is not configured to support act(...)
    (pass) VncPanel render > reconnect button refetches the signed URL [0.77ms]
    
    5 tests failed:
    (fail) StageInsightsSidebar > renders permission badge for read-only [1.03ms]
    (fail) StageInsightsSidebar > renders permission badge for full access [1.03ms]
    (fail) StageInsightsSidebar > renders projected agent tool names, descriptions, categories, and invoked state [1.21ms]
    (fail) StageInsightsSidebar > legacy stages without agent tools keep permission fallback only [0.78ms]
    (fail) StageInsightsSidebar > renders empty-friendly content when stage projection is missing [0.78ms]
    
     484 pass
     5 fail
     1189 expect() calls
    Ran 489 tests across 59 files. [10.06s]
    error: script "test" exited with code 1
    ```

## Context
- failure_class: deterministic
- failure_signature: verify|deterministic|script failed with exit code: <n> ## output cespanelview > shows api error state with the error message [<n>.73ms] react-test-renderer is deprecated. see https://react.dev/warnings/react-test-renderer the current testing environment is not 


The verify step failed. Read the build output from context and fix all format, clippy, Rust test, docs, TypeScript typecheck/test, and build failures.