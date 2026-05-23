All verify steps pass:
- Merge with origin/main: clean
- Format check: clean
- Docs refresh: up-to-date
- Banned auth patterns: none
- Clippy: clean
- Tests: 6273 passed, 181 skipped
- Docs check: up-to-date
- bun install / typecheck (web + api-client) / web tests: all pass
- Release build (`-p fabro-cli`): success

The verify failure root cause was missing local git identity for the merge commit. I configured `user.email`/`user.name` and re-ran every verify step end-to-end.