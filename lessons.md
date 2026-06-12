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