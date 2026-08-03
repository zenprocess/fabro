//! `fabro-sandbox-forkd` — JSON-RPC 2.0 over stdio sandbox-provider plugin
//! for the forkd microVM controller.
//!
//! Operational model: the host (fabro-server / fabro-cli) spawns this
//! binary as a subprocess when the operator selects the `forkd` plugin
//! provider.  The host writes JSON-RPC 2.0 requests, one per line, on
//! stdin; this process reads them, dispatches to forkd, and writes the
//! responses, one per line, on stdout.  **stdout is the protocol channel
//! — do not write anything else to it.**  All logs go to stderr.
//!
//! The plugin process handles one sandbox at a time (it is a
//! single-tenant subprocess).  When the host tears down a sandbox and
//! the operator is done, it sends a `shutdown` notification; the process
//! exits 0.

use std::sync::Arc;

use anyhow::{Context, Result};
use fabro_sandbox_forkd::{Plugin, PluginState, default_handler};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Logs go to stderr (stdout is the JSON-RPC protocol channel).
    // RUST_LOG controls verbosity; default is `warn` to keep the protocol
    // channel clean.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    // The plugin's stdout is the JSON-RPC protocol channel; logs MUST
    // go to stderr.  This is the documented boundary for plugin
    // subprocesses, not a process-wide env mutation.  The closure is the
    // `MakeWriter` impl `tracing_subscriber` requires.
    #[expect(
        clippy::disallowed_methods,
        reason = "Plugin subprocess writes logs to stderr because stdout is the protocol channel; this is the documented boundary, not a process-wide env mutation."
    )]
    let stderr_writer = || std::io::stderr();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(stderr_writer)
        .with_ansi(false)
        .init();

    let state = Arc::new(PluginState::from_env());
    let handler = default_handler();
    let plugin = Plugin::new(state, handler);
    plugin
        .run()
        .await
        .with_context(|| "fabro-sandbox-forkd: plugin loop terminated")?;
    Ok(())
}
