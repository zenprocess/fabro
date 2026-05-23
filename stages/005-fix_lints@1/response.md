Clippy passes cleanly now. Summary of fixes:

1. **`lib/crates/fabro-sandbox/src/docker.rs`** — moved `use std::io::Read as _;` (with its `#[expect(clippy::disallowed_types, ...)]` attribute) to the top of `download_file_bytes`, before any statements, to satisfy `clippy::items_after_statements`.

2. **`lib/crates/fabro-agent/src/apply_patch.rs`** — replaced four `std::fs::{write, read_to_string}` test calls (disallowed in Tokio paths) with `tokio::fs` equivalents, added `use tokio::fs;` to the test module, and rewrote the calls as `fs::…` to also satisfy `clippy::absolute_paths`.