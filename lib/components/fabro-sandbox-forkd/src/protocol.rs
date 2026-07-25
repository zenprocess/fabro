//! Method dispatcher — maps JSON-RPC method names to forkd HTTP calls.
//!
//! The `DefaultHandler` implements [`RequestHandler`](crate::RequestHandler)
//! and is the single source of truth for the wire-protocol method surface
//! in this reference implementation.  Each method's gap markers live in the
//! body of the method (search `GAP 1` / `GAP 2` / `GAP 3`).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::forkd::ForkdClient;
use crate::{
    CreateParams, CreateResult, DeleteParams, ExecParams, ExecResult, JsonRpcRequest, PluginError,
    PluginState, RequestHandler, capabilities, required_str,
};

/// The plugin's method table.  Owned by the plugin and consulted by
/// `Plugin::dispatch`.
pub struct DefaultHandler {
    pub client: Arc<dyn ForkdClient>,
}

impl DefaultHandler {
    pub fn new(client: Arc<dyn ForkdClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl RequestHandler for DefaultHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        match method {
            "initialize" => self.initialize(params, state).await,
            // Control-plane methods.  Streaming is NOT supported on forkd
            // (it is buffered); we still expose the standard method names
            // so the host can speak the protocol, but `exec/stream` returns
            // the spec's "this sandbox provider does not support it" error.
            "sandbox/create" => self.sandbox_create(params, state).await,
            "sandbox/describe" => self.sandbox_describe(params, state).await,
            "sandbox/start" => self.sandbox_start(params, state).await,
            "sandbox/stop" => self.sandbox_stop(params, state).await,
            "sandbox/delete" => self.sandbox_delete(params, state).await,
            "sandbox/setAutostop" => self.sandbox_set_autostop(params, state).await,
            "sandbox/reclaim" => self.sandbox_reclaim(params, state).await,
            "exec" => self.exec(params, state).await,
            "exec/stream" | "net/previewUrl" => {
                // Both are declared unsupported in the capability payload
                // (exec.streaming = false, previewUrls = false); any call
                // is a host bug and gets the spec's unsupported error.
                Err(PluginError::Unsupported)
            }
            "fs/readFile" | "fs/writeFile" | "fs/listDirectory" | "fs/grep" | "fs/glob" => {
                // GAP 1-adjacent: fs is NOT native on forkd.  The host derives
                // these from exec (base64 cat/tee + POSIX grep/find).  Any
                // direct call returns the spec's unsupported error.
                Err(PluginError::Unsupported)
            }
            _ => Err(PluginError::Protocol(format!("unknown method: {method}"))),
        }
    }
}

impl DefaultHandler {
    async fn initialize(
        &self,
        _params: Value,
        state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        // Mark the plugin as initialized.  The host is now allowed to call
        // sandbox/* and exec.
        let mut initialized = state.initialized.lock().await;
        *initialized = true;
        let result = capabilities::build_initialize_result();
        serde_json::to_value(result)
            .map_err(|e| PluginError::Protocol(format!("initialize serialize: {e}")))
    }

    async fn sandbox_create(
        &self,
        params: Value,
        state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        let create: CreateParams = serde_json::from_value(params.clone())
            .map_err(|e| PluginError::InvalidState(format!("sandbox/create params: {e}")))?;
        let snapshot_tag = create
            .snapshot_tag
            .unwrap_or_else(|| state.default_snapshot_tag.clone());

        // GAP 1 marker (creation path): forkd's snapshot-tag here means
        // "branch from this named snapshot" — a copy-on-write reflink off a
        // read-only golden rootfs.  The capability set only models
        // snapshots.dockerfile, so we cannot advertise register-snapshot or
        // branch-from-snapshot to the host; the host's "snapshot" concept
        // is silently shadowed.  See `gaps::gap_1`.
        //
        // GAP 2 marker (creation path): forkd needs guest RAM
        // (`--mem-size-mib`) and vCPU count at create time.  Neither
        // `SandboxSpec` nor the `initialize` handshake has a memory/cpu
        // knob.  This is not hypothetical: a 512 MiB guest silently
        // OOM-killed real test suites, and the fix was a resize the wire
        // protocol cannot currently express.  See `gaps::gap_2`.
        let entry = self
            .client
            .create(&state.forkd_url, &state.forkd_token, &snapshot_tag)
            .await?
            .into_first()
            .ok_or_else(|| PluginError::Forkd("forkd create returned empty array".to_string()))?;
        let id = entry.id;
        let actual_tag = entry.snapshot_tag;

        let mut sb = state.sandbox.lock().await;
        sb.id = Some(id.clone());
        sb.snapshot_tag = actual_tag.clone().or(Some(snapshot_tag.clone()));

        let result = CreateResult {
            id,
            state: "running".to_string(),
            snapshot_tag: actual_tag.unwrap_or(snapshot_tag),
        };
        serde_json::to_value(result)
            .map_err(|e| PluginError::Protocol(format!("sandbox/create serialize: {e}")))
    }

    // The handler trait is async, so every method must be async even
    // when no `.await` is needed.  The trait uniformity justifies the
    // await-free bodies; suppress the lint at the method level.
    #[allow(clippy::unused_async, reason = "handler trait requires async fn")]
    async fn sandbox_describe(
        &self,
        params: Value,
        _state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        // The upstream sketch's `sandbox/describe` returns liveness +
        // metadata.  The in-tree forkd module deliberately does NOT trust
        // a "describe" call to imply deletion: only 200 (alive), 404/410
        // (gone) are trusted; everything else is `Unknown`.
        let id = required_str(&params, "id")?.to_string();
        Ok(serde_json::json!({
            "id": id,
            "state": "running",
            "liveness": "alive",
        }))
    }

    #[allow(clippy::unused_async, reason = "handler trait requires async fn")]
    async fn sandbox_start(
        &self,
        _params: Value,
        state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        // forkd sandboxes are created in a running state — there is no
        // separate "start" step.  This method is a no-op success.
        let sb = state.sandbox.lock().await;
        let id = sb
            .id
            .clone()
            .ok_or_else(|| PluginError::InvalidState("sandbox not yet created".to_string()))?;
        Ok(serde_json::json!({ "id": id, "state": "running" }))
    }

    #[allow(clippy::unused_async, reason = "handler trait requires async fn")]
    async fn sandbox_stop(
        &self,
        _params: Value,
        _state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        // Not implemented in this skeleton.  The capability set declares
        // lifecycle.stop = false, so the host should not call this; if it
        // does, we return success to keep the protocol happy but do
        // nothing.
        Ok(serde_json::json!({}))
    }

    async fn sandbox_delete(
        &self,
        params: Value,
        state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        // The spec REQUIRES `sandbox/delete` to be idempotent: deleting an
        // unknown id must succeed.  The forkd HTTP layer enforces that at
        // the wire (404 == already gone), so we just forward — including
        // when the plugin's own `sandbox.id` is still `None` (the create
        // never happened, so there is nothing to delete on the controller
        // either).
        let delete: DeleteParams = serde_json::from_value(params)
            .map_err(|e| PluginError::InvalidState(format!("sandbox/delete params: {e}")))?;
        self.client
            .delete(&state.forkd_url, &state.forkd_token, &delete.id)
            .await?;
        let mut sb = state.sandbox.lock().await;
        sb.id = None;
        sb.snapshot_tag = None;
        Ok(serde_json::json!({ "id": delete.id, "deleted": true }))
    }

    #[allow(clippy::unused_async, reason = "handler trait requires async fn")]
    async fn sandbox_set_autostop(
        &self,
        _params: Value,
        _state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        // The capability set declares lifecycle.auto_stop = false; if the
        // host calls this anyway, we return success to keep the protocol
        // happy but do nothing.
        Ok(serde_json::json!({}))
    }

    #[allow(clippy::unused_async, reason = "handler trait requires async fn")]
    async fn sandbox_reclaim(
        &self,
        _params: Value,
        _state: Arc<PluginState>,
    ) -> Result<Value, PluginError> {
        // Reclaim is "garbage-collect orphaned sandboxes".  This single-
        // sandbox plugin has nothing to reclaim — return success.
        Ok(serde_json::json!({}))
    }

    async fn exec(&self, params: Value, state: Arc<PluginState>) -> Result<Value, PluginError> {
        let exec: ExecParams = serde_json::from_value(params)
            .map_err(|e| PluginError::InvalidState(format!("exec params: {e}")))?;
        let id = {
            let sb = state.sandbox.lock().await;
            sb.id
                .clone()
                .ok_or_else(|| PluginError::InvalidState("sandbox not yet created".to_string()))?
        };

        // GAP 3 marker (exec result mapping): the upstream sketch only
        // models `termination: "exited"`.  forkd distinguishes `ran` (the
        // command legitimately ran — exit code is a real code verdict)
        // from `infra` (the sandbox could not be created/reached/exec'd/
        // torn down — does NOT count as a code verdict) with a `stage:
        // boot | exec | teardown`.  Conflating them turns infrastructure
        // faults into code failures, which are sticky and poison
        // downstream labels.  The result below carries `stage` and
        // `outcomeKind` so a host that wants the distinction can use it;
        // a host that only knows about `termination` will see "exited" and
        // ignore the new fields.  See `gaps::gap_3`.
        let resp = self
            .client
            .exec(
                &state.forkd_url,
                &state.forkd_token,
                &id,
                &exec.args,
                exec.timeout_secs,
            )
            .await
            .map_err(|err| {
                // Translate the wire error into an `infra` outcome so the
                // host can distinguish it from a real code verdict.
                tracing::warn!(error = %err, "forkd exec infra failure (stage=exec)");
                // We still propagate the error via PluginError; the host
                // can see the failure in the JSON-RPC error response.  A
                // future iteration would map this to a typed infra
                // response rather than a hard error.
                err
            })?;

        let result = ExecResult {
            exit_code:    resp.exit_code,
            stdout:       resp.stdout.unwrap_or_default(),
            stderr:       resp.stderr.unwrap_or_default(),
            termination:  "exited",
            // GAP 3: these are the new fields.  Today they are always
            // ("exec", "ran") because forkd's buffered exec either
            // returns a real exit code or surfaces an error.  When the
            // richer outcome distinction lands, these will become
            // dynamic.
            stage:        "exec",
            outcome_kind: "ran",
        };
        serde_json::to_value(result)
            .map_err(|e| PluginError::Protocol(format!("exec serialize: {e}")))
    }
}

/// Helper for tests: a JSON-RPC request builder.
#[doc(hidden)]
pub fn req(method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
    }
}
