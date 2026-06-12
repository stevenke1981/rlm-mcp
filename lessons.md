# lessons.md — rlm-mcp

---
## Lesson #1 — 2026-06-12
**Trigger:** Child sub-task ignored per-call budget override; used default max_depth=4 instead of tree budget.
**Rule:** When creating child tasks, always inherit budget from the persisted TaskTree, not from optional per-call overrides.
**Source:** P1 recursive sub-call runtime

---
## Lesson #2 — 2026-06-12
**Trigger:** PowerShell compound commands with `$var =` fail when shell wrapper prepends `(cd && ...)`.
**Rule:** For Windows CLI smoke tests, use single-invocation commands without inline variable assignment, or run via `cargo test` instead.
**Source:** CLI verification

---
## Lesson #3 — 2026-06-12
**Trigger:** `PeekOptions` moved into `peek_session`, then `opts.query` used after move in trajectory recorder.
**Rule:** Capture trajectory byte counts from inputs before calling functions that take ownership of option structs.
**Source:** P1 trajectory logging

---
## Lesson #4 — 2026-06-12
**Trigger:** Budget check used `current >= max` so reading exactly up to the limit was rejected.
**Rule:** Session budget pre-checks must use projected usage (`current + increment > max`), not `>=` on the limit itself.
**Source:** P1 cost/budget accounting

---
## Lesson #5 — 2026-06-12
**Trigger:** S-NIAH benchmark compared trajectory `bytes_in` (includes full scan load) against direct baseline; peek with `include_content` returned entire chunk.
**Rule:** Benchmark cost metrics must use model-visible context bytes (peek previews / chunk evidence), not external storage load or full-chunk content when filtering.
**Source:** P1 benchmark mini-suite

---
## Lesson #6 — 2026-06-12
**Trigger:** Project still used `codebase-memory-rlm-mcp` crate/binary name while repo directory is `rlm-mcp`.
**Rule:** When renaming the project, update Cargo.toml package/lib/bin names, SERVER_NAME, default cache dir, install scripts, and all MCP templates in one commit before release.
**Source:** project rename to rlm-mcp

---
## Lesson #7 — 2026-06-12
**Trigger:** CLI integration tests failed because `env!("CARGO_BIN_EXE_rlm_mcp")` is not set at compile time on this toolchain.
**Rule:** Process-level CLI tests should resolve the binary via `std::env::var("CARGO_BIN_EXE_*")` with fallback to `target/<profile>/rlm-mcp(.exe)`.
**Source:** MCP/CLI contract tests

---
## Lesson #8 — 2026-06-12
**Trigger:** Cross-process session reads failed when a second `SessionStore` was created before the first write finished.
**Rule:** Use `get_or_hydrate` on read paths and tombstone+per-session lock on writes; never assume in-memory map is complete across MCP/CLI invocations.
**Source:** P1 session storage and concurrency

---
## Lesson #9 — 2026-06-12
**Trigger:** `tools_list_matches_snapshot` failed after expanding `rlm_task_create` provider enum without refreshing the packaging snapshot.
**Rule:** After changing MCP tool schemas, run `cargo test write_tools_snapshot -- --ignored` and commit `packaging/mcp/tools-list.snapshot.json` in the same commit.
**Source:** P2 provider abstraction