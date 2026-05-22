Review/cleanup complete.

What I did:
- Compared the branch diff against `origin/main`.
- Launched the three parallel review agents for reuse, quality, and efficiency.
- Investigated the main actionable-looking finding around `FABRO_WORKER_AGENT_RUN_TOOLS` default env behavior.
  - Tried tightening the test to expect explicit removal.
  - Validation showed `tokio::process::Command`/test helper reports that case as `Unchanged`, even though `worker_command()` still calls `env_remove(...)` before spawning.
  - Reverted the attempted test change; no source changes remain.
- Confirmed the env-gated CLI behavior matches the provided plan, so I did not revert it to config-only behavior.

Validation run:
- `cargo nextest run -p fabro-server worker_command` ✅
- `cargo nextest run -p fabro-cli fabro_run_tools_enabled_env_requires_true` ✅
- `cargo nextest run -p fabro-config run_agent_fabro_tools` ✅

Final state:
- Working tree is clean.
- No cleanup commits were made.
- Branch diff remains the existing implementation diff:
  - `fabro-cli/src/commands/run/runner.rs`
  - `fabro-server/src/server.rs`
  - `fabro-server/src/server/tests.rs`
  - `fabro-static/src/env_vars.rs`