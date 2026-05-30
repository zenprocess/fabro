Implemented/verified the automation master `enabled` gate removal.

Changes made in this turn:
- `apps/fabro-web/app/routes/automation-detail.tsx`
  - Run button title now uses the trigger-derived condition exactly: `!apiTrigger?.enabled`.
- `apps/fabro-web/app/routes/automations.tsx`
  - `AutomationCard` now receives a dedicated run-button `disabled` prop.
  - Run-button disabled state now follows the plan expression:
    `deleting || !automation.apiEnabled || (runningId !== null && runningId !== automation.id)`.

The Rust domain/API/server/generated-client portions were already aligned with the plan in the checked-out tree:
- No top-level automation `enabled` in domain structs/TOML/OpenAPI/generated client.
- Legacy top-level TOML `enabled` rejection test exists and passes.
- API run creation checks only for an enabled API trigger.
- Conflict detail is `automation has no enabled API trigger`.

Verification run:
- `cargo nextest run -p fabro-automation` ✅
- `cargo build -p fabro-api` ✅
- `cargo nextest run -p fabro-api --test automation_round_trip` ✅
- `cargo nextest run -p fabro-server --features test-support --test it api::automations` ✅
- `cd lib/packages/fabro-api-client && bun run generate` ✅
- `cd lib/packages/fabro-api-client && bun run typecheck` ✅
- `cd apps/fabro-web && bun run typecheck` ✅
- `cargo +nightly-2026-04-14 fmt --check --all` ✅

Notes:
- The plan’s literal `cargo nextest run -p fabro-api automation_round_trip` and `cargo nextest run -p fabro-server automations` matched zero tests under this repo’s nextest setup, so I ran the intended test targets explicitly as shown above.
- The leftover-reference search only reports trigger-level `enabled` fixtures and `enabled_api_trigger()` method usage; no removed top-level automation master gate remains.
- TypeScript client generation initially required Java; I installed a headless JRE in the sandbox and reran generation successfully.