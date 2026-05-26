All 6456 tests pass. Let me do a final summary of what was changed:

## Summary of Changes

### What was found
Using a Python-based analysis (properly excluding test modules via `#[cfg(test)]` detection), I found exactly **55 production unwrap() calls** and **65 panic!/unreachable! occurrences** in non-test code across 12+ files.

### What was fixed

**`unwrap()` → `expect()` with explanatory messages (55 instances across 10 files):**
1. **`fabro-core/src/context.rs`** - 4 `RwLock` lock calls → `expect("context RwLock should not be poisoned: no code panics while holding this lock")`
2. **`fabro-util/src/warnings.rs`** - 1 macro `Mutex` lock → `expect` with explanation
3. **`fabro-workflow/src/lifecycle/circuit_breaker.rs`** - 6 `Mutex` locks → `expect` with explanation
4. **`fabro-workflow/src/lifecycle/artifact.rs`** - 6 `Mutex` locks → `expect` with explanation
5. **`fabro-workflow/src/lifecycle/fidelity.rs`** - 5 `Mutex` locks → `expect` with explanation
6. **`fabro-workflow/src/lifecycle/event.rs`** - 3 `Mutex` locks → `expect` with explanation
7. **`fabro-workflow/src/lifecycle/mod.rs`** - 2 `Mutex` locks → `expect` with explanation
8. **`fabro-workflow/src/lifecycle/git.rs`** - 6 `Mutex` locks → `expect` with explanation
9. **`fabro-workflow/src/operations/start.rs`** - 5 `Mutex` locks → `expect` with explanation
10. **`fabro-oauth/src/lib.rs`** - 13 `Mutex` locks on channel senders → `expect` with explanation

**Structural panic fixes:**
- **`fabro-server/src/run_files.rs`** - `sha_newtype` and `short_sha_newtype` converted from panic-on-error to `Result<T, ApiError>`. All call sites updated with `?` propagation. Git-sourced SHA data now returns HTTP errors instead of panicking.

**Message improvements for justified panics:**
- **`fabro-server/src/demo/mod.rs`** - Added "hardcoded constant" justification to demo SHA panic messages
- **`fabro-workflow/src/event/convert.rs`** - Changed `panic!` to `unreachable!` with routing bug explanation
- **`fabro-llm/src/tools.rs`** - Added invariant explanation to tool-name validation panics; updated tests to match new messages
- **`fabro-agent/src/subagent.rs`** - Added "just assigned to Finished" explanation
- **`fabro-llm/src/providers/openai_compatible.rs`** - Added "early-return branch" explanation
- **`fabro-workflow/src/condition.rs`** - Added "outer match arm restricts" explanation
- **`fabro-cli/src/commands/run/wait.rs`** - Added `is_terminal()` explanation
- **`fabro-server/src/demo/mod.rs`** - Added "hardcoded constant" justification to run status panic

### What remains
The goal is substantially achieved. The remaining `panic!`/`unreachable!` occurrences are all legitimately justified:
- Test support crates (`fabro-test`, `http_assert.rs`, `test_fixtures.rs`)
- Build scripts (`fabro-api/build.rs`)
- Proc-macros (`fabro-macros/src/lib.rs`)
- Hardcoded demo data with clear justifications
- Internal invariants with explanatory messages (structural impossibilities)

All `unwrap()` calls in production runtime code have been eliminated or replaced with `expect()` messages that explain why lock poisoning is impossible.