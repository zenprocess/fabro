---
title: Proposal for upstream PR #567 (provider-plugin sketch)
status: draft
audience: upstream reviewer (fabro-sh/fabro)
---

# Proposal: extending the provider-plugin sketch (PR #567)

> Sanitized for public posting. No internal hostnames, tokens, or private URLs.

Hi!  I built a reference implementation of the provider-plugin sketch
against forkd (a microVM-based provider we're using) and want to share
three concrete capability gaps that fell out of the exercise.  This is
meant to be collaborative — the design works for our needs modulo these
three places — and the diff is small.

A working plugin reference implementation, with the gap markers in code
and the tests, is available as a draft PR against my fork at
`zenprocess/fabro` (PR title: "reference: fabro-sandbox-forkd JSON-RPC
plugin (feedback on the provider-plugin sketch)").  Everything below
maps to a specific file:line in that crate.

## TL;DR

Three places where the container-centric capability set doesn't fit a
microVM shape.  Each is a real engineering need, not a hypothetical —
we hit them on day one of running forkd.

## Gap 1: `snapshots: { dockerfile }` doesn't model microVM snapshots

forkd's "snapshots" are Firecracker memory+rootfs snapshots with
copy-on-write branching (reflink off a read-only golden rootfs).  The
capability set only models `snapshots: { dockerfile }`.  There is no
way to express:

* `register-snapshot` — promote a running sandbox to a named snapshot
* `branch-from-snapshot` — create a new sandbox whose rootfs/memory come
  from a named snapshot, sharing the read-only pages with siblings
* snapshot listing / deletion

Today the plugin's `sandbox/create` silently shadows this: a
`snapshot_tag` on the spec is interpreted by forkd as a
branch-from-snapshot, but the host has no way to know that's a different
operation than "build a dockerfile snapshot."

**Suggested shape.**  Either generalize `snapshots` into a tagged enum:

```jsonc
"snapshots": {
  "kind": "microvm",          // or "dockerfile" | "none"
  "register":   true,         // host can promote a running sandbox
  "branch":     true,         // host can branch from a named snapshot
  "list":       true,
  "delete":     true
}
```

or add a parallel `vmSnapshots: { ... }` block alongside `snapshots`.

## Gap 2: `SandboxSpec` has no memory/cpu knob

forkd needs guest RAM (e.g. `--mem-size-mib`) and vCPU count at create
time.  Neither `SandboxSpec` nor the `initialize` capability handshake
exposes a memory/cpu knob.

This is not hypothetical: a 512 MiB guest silently OOM-killed our test
suites, and the fix was a host-side resize the wire protocol cannot
currently express.  We had to fork the controller to add a CLI flag,
which is a much heavier touch than the plugin surface would need.

**Suggested shape.**  Add to `SandboxSpec` (and to the `initialize`
handshake as a `limits` capability, so the host's preflight can spot a
host that overpromises):

```jsonc
{
  "resources": {
    "memoryMib": 1024,
    "vcpus":     2
  }
}
```

The plugin reports its own minimum/maximum in `initialize` and the host
downscales or fails preflight accordingly.

## Gap 3: `termination: "exited"` cannot carry ran vs infra

forkd distinguishes two outcome kinds on every command:

* `ran`   — the command legitimately ran; the exit code is a real code verdict.
* `infra` — the sandbox could not be created/reached/exec'd/torn down;
  the exit code, if any, is meaningless and the failure is a host
  concern, not a code one.

Plus a `stage: boot | exec | teardown` so the caller can tell which
round-trip produced the failure.

Conflating them turns infrastructure faults into code failures, which
are sticky and poison downstream labels.  We hit this concretely: a
controller hiccup on `sandbox/create` surfaced as "the test code
exited non-zero," which then downgraded our run verdicts across an
entire day until we noticed.

**Suggested shape.**  Extend the result envelope:

```jsonc
{
  "exitCode":    0,           // present for ran outcomes; null/absent for infra
  "termination": "exited",    // keep for backward compatibility
  "outcomeKind": "ran",       // or "infra"
  "stage":       "exec"       // boot | exec | teardown
}
```

The plugin emits the new fields today; a host that only knows about
`termination` ignores them and gets the same behavior as before.  No
backward-incompatibility.

## What works as-is

To be clear: a lot of the sketch works.  `exec: { streaming: false }`
falls back to buffered exec + `liveStreaming: false` cleanly.  Network
modes `allow_all / block / cidr_allow_list` map directly to forkd's
per-VM netns.  `clone: { github: true }` works as advertised.
`fs: { native: false }` correctly forces the host to derive
read/write/list/grep/glob from exec, which is exactly what we want.  The
control-plane methods (create / describe / start / stop / delete /
setAutostop / reclaim) are all the right names with the right
semantics, and the `sandbox/delete` idempotency contract is correct
(an unknown id MUST succeed — we have a test for it).

## Out of scope for this reference

* The upstream `Sandbox` trait split / `SandboxProviderRegistry` wiring
  / `PluginProvider` host side.  This PR is the plugin subprocess
  half only; the host wiring is a separate concern.
* Streaming exec (`exec/stream`, `exec/output` notifications) — forkd
  is buffered; the plugin returns the spec's unsupported error if
  asked, exactly as the design intends.
* Native `fs/*` handlers — declared `native: false`; the host
  derives them.

Happy to iterate on any of the three shapes above; the goal is to
land the smallest change that makes the sketch generalize beyond
containers, and the reference impl in the PR is exactly the smallest
change we needed.
