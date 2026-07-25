//! `initialize` capability payload.
//!
//! This is where forkd's microVM shape first meets the upstream capability
//! set.  Every value below is **honest** — a `true` here is a contract with
//! the host's preflight.  See `gaps` for the three places where the
//! capability set is too narrow for forkd.

use serde::Serialize;

/// The version of the JSON-RPC 2.0 provider-plugin protocol this plugin
/// implements.  Bump when the wire surface changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// For the test suite — exported only for `pub use` in `lib.rs`.  The test
/// harness asserts the EXACT honest values returned to the host.
#[derive(Debug, Serialize, PartialEq)]
pub struct InitializeResult {
    pub protocol_version: u32,
    pub provider: ProviderInfo,
    pub capabilities: Capabilities,
    pub limits: Limits,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ProviderInfo {
    pub kind: &'static str,
    pub version: &'static str,
    pub display_name: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Capabilities {
    pub exec: ExecCapability,
    pub stdio: bool,
    pub fs: FsCapability,
    pub grep: bool,
    pub glob: bool,
    pub preview_urls: bool,
    pub snapshots: SnapshotCapability,
    pub network: NetworkCapability,
    pub lifecycle: LifecycleCapability,
    pub clone: CloneCapability,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ExecCapability {
    /// GAP-adjacent: forkd's controller exec is **buffered**, not streamed.
    /// `false` here tells the host to use the buffered code path (and to set
    /// `liveStreaming:false` in `exec/run`).  Declaring `true` would be a
    /// lie that breaks the host's preflight contract.
    pub streaming: bool,
    pub cancel: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FsCapability {
    /// `false` — forkd has no native fs ops; the host derives read/write/
    /// list/grep/glob from exec (base64 cat/tee + POSIX grep/find).
    pub native: bool,
    /// Whether the plugin can upload a file directly (not via exec).  forkd
    /// cannot — it goes through `exec` with `tee`.
    pub upload: bool,
    /// Whether the plugin can download a file directly (not via exec).  forkd
    /// cannot — it goes through `exec` with `cat | base64`.
    pub download: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SnapshotCapability {
    /// GAP 1: forkd snapshots are Firecracker memory+rootfs snapshots with
    /// copy-on-write branching (reflink off a read-only golden rootfs).  The
    /// upstream capability set only models dockerfile snapshots — there is
    /// no way to express register-snapshot or branch-from-snapshot.  We
    /// declare `false` because we have no dockerfile snapshot path.  See
    /// `gaps::gap_1`.
    pub dockerfile: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct NetworkCapability {
    /// forkd controls per-VM netns; these three modes map directly.
    pub modes: Vec<&'static str>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct LifecycleCapability {
    pub stop: bool,
    pub auto_stop: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CloneCapability {
    /// forkd does an in-VM sparse git clone against the GitHub origin.
    pub github: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Limits {
    /// 4 MiB — matches the upstream sketch.
    pub max_message_bytes: u32,
}

/// Build the **honest** capability payload for this plugin.
pub fn build_initialize_result() -> InitializeResult {
    InitializeResult {
        protocol_version: PROTOCOL_VERSION,
        provider: ProviderInfo {
            kind: "forkd",
            // The forkd controller version this skeleton targets.  Test
            // should not pin this; bump when the wire shape changes.
            version: "0.1.0",
            display_name: "forkd microVM sandbox provider",
        },
        capabilities: Capabilities {
            // GAP-adjacent: see `ExecCapability::streaming`.  Buffer-only.
            exec: ExecCapability { streaming: false, cancel: false },
            stdio: false,
            fs: FsCapability { native: false, upload: false, download: false },
            grep: false,
            glob: false,
            preview_urls: false,
            // GAP 1 marker: see `SnapshotCapability::dockerfile` and
            // `gaps::gap_1`.  No dockerfile snapshot path on forkd.
            snapshots: SnapshotCapability { dockerfile: false },
            network: NetworkCapability {
                modes: vec!["allow_all", "block", "cidr_allow_list"],
            },
            lifecycle: LifecycleCapability { stop: false, auto_stop: false },
            clone: CloneCapability { github: true },
        },
        limits: Limits { max_message_bytes: 4 * 1024 * 1024 },
    }
}
