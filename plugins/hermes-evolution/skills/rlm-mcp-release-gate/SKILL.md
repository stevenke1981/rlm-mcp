---
name: rlm-mcp-release-gate
description: Verify, package, and hand off rlm-mcp changes. Use when working in the rlm-mcp repo before committing, pushing, releasing, changing MCP tools or schemas, editing installer/package files, or preparing agent handoff notes.
---

# rlm-mcp Release Gate

Use this skill inside the `rlm-mcp` repository when changes might affect Rust code, MCP tool contracts, installers, packaging, docs, skills, or hooks.

## Inspect

1. Run `git status --short --branch`.
2. Read the files that own the touched surface before editing.
3. Check `lessons.md` for repo-specific failure modes.
4. If MCP tools or schemas changed, inspect `packaging/mcp/tools-list.snapshot.json`.

## Verify

Run the narrowest useful command first, then expand:

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

If a tool schema changed, refresh and commit the snapshot in the same change:

```powershell
cargo test write_tools_snapshot -- --ignored
```

## Release Smoke

Before claiming release readiness:

```powershell
cargo build --release
cargo test --test release_smoke --release
.\scripts\package-release.ps1
```

On Unix, use `./scripts/package-release.sh` after `cargo build --release`.

## Commit Handoff

Before commit or push, summarize:

1. What changed.
2. Why it changed.
3. Verification commands and results.
4. Whether the task produced a reusable lesson for `lessons.md`, a skill, or a hook.
