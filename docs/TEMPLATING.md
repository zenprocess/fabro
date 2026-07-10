# Run variable interpolation (`{{ vars.* }}`)

Full contract: [`docs/public/workflows/variables.mdx`](public/workflows/variables.mdx) (merged
from upstream fabro-sh/fabro #524, #513, #492 — this file only adds a fork-local pointer +
migration example, it does not duplicate the upstream doc).

## Quick reference

- Templates render only in the graph `goal` and node `prompt` attributes. Every other
  attribute (`script`, `label`, `model`, `provider`, `condition`, edge attrs) is literal text.
- `{{ goal }}` — the workflow goal.
- `{{ inputs.NAME }}` — a typed value from `[run.inputs]` (TOML scalar; overridable with
  `-I name=value` / `--input name=value`).
- `{{ vars.NAME }}` — a server-managed run variable (string), set with
  `fabro variable set NAME value` and read back with `fabro variable get/list`. Non-sensitive
  values only — use `fabro secret set` for credentials.
- Referencing an unknown `inputs.*` or `vars.*` member is a strict render error (not silently
  empty) outside of structural/lenient passes.
- `{{ env.* }}` is available in config strings and HTTP hook headers, but NOT in `goal`/`prompt`.

Implementation: `lib/crates/fabro-template/src/lib.rs` (`TemplateContext::with_vars`,
`RenderContext`).

## Migration note — qa-pipeline goal line

Our qa-pipeline config predates #524 and used shell-style `$repo_name` / `$sha` placeholders,
which fabro's template renderer never interpolated (they rendered as literal text). Post-merge,
the equivalent goal line uses the actual `{{ vars.* }}` syntax:

```toml
# before (never interpolated — rendered as literal text)
goal = "QA gate: tests + AI review for $repo_name @ $sha"

# after (renders vars.repo_name / vars.sha set via `fabro variable set`)
goal = "QA gate: tests + AI review for {{ vars.repo_name }} @ {{ vars.sha }}"
```

`repo_name` and `sha` must exist as server-managed variables (`fabro variable set repo_name ...`,
`fabro variable set sha ...`) before the run starts, or the render fails with an undefined-variable
error per the strict-mode contract above.
