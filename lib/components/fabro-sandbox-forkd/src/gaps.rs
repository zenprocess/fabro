//! The three capability gaps that the container-centric upstream sketch
//! misses when the sandbox is a microVM.  Marked at the friction site in
//! code (search `GAP 1` / `GAP 2` / `GAP 3`) and explained here so a
//! reviewer can find them without reading the rest of the crate.

/// GAP 1 — microVM snapshot / COW-branch has no capability.
///
/// forkd's "snapshots" are Firecracker memory+rootfs snapshots with
/// copy-on-write branching (reflink off a read-only golden rootfs).  The
/// upstream sketch only models `snapshots: { dockerfile }`.  There is no
/// way to express:
/// * register-snapshot  — promote a running sandbox to a named snapshot
/// * branch-from-snapshot — create a new sandbox whose rootfs/memory come
///   from a named snapshot, sharing the read-only pages with siblings
/// * snapshot listing / deletion
///
/// GAP 2 — guest resource sizing has no home in `SandboxSpec`.
///
/// forkd needs guest RAM (`--mem-size-mib`) and vCPU count at create time.
/// Neither the upstream `SandboxSpec` nor the `initialize` capability
/// handshake exposes a memory/cpu knob.  This is not hypothetical: a 512
/// MiB guest silently OOM-killed real test suites on this very deployment,
/// and the fix was a resize the wire protocol cannot currently express.
///
/// GAP 3 — the ran-vs-infra outcome distinction is richer than
/// `{termination}`.
///
/// forkd distinguishes `ran` (the command completed — exit code is a real
/// code verdict) from `infra` (the sandbox could not be
/// created/reached/exec'd/torn down) with a `stage: boot | exec |
/// teardown`.  The upstream `termination: "exited"` cannot carry that
/// information.  Conflating them makes infrastructure faults post as code
/// failures, which are sticky and poison downstream labels.
pub fn gap_1() -> &'static str {
    "microVM snapshot/COW-branch: no register-snapshot / branch-from-snapshot capability"
}

pub fn gap_2() -> &'static str {
    "guest resource sizing: no memory / vCPU knob in SandboxSpec or initialize handshake"
}

pub fn gap_3() -> &'static str {
    "ran-vs-infra outcome distinction: richer than {termination} — needs stage + outcomeKind"
}
