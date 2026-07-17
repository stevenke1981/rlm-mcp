# Extending rlm-mcp (providers, transforms, plugins)

This is the lightweight “plugin formalization” guide from the project analysis
roadmap. rlm-mcp does **not** load dynamic libraries at runtime. Extension points
are env-configured providers, built-in transform ops, and optional separate
binaries (e.g. Hermes release gate).

## 1. Custom sub-task providers

Recursive tasks (`rlm_task_create`) use a provider trait:

| Provider | When | Config |
|----------|------|--------|
| `mock` | Offline tests, deterministic | Default for tests |
| `dry-run` | Plan without model calls | `provider=dry-run` |
| `openai` | OpenAI-compatible HTTP API | `RLM_OPENAI_API_KEY`, `RLM_OPENAI_BASE_URL`, `RLM_ALLOW_NETWORK=1` |
| `command` | Local executable worker | `RLM_PROVIDER_COMMAND`, optional sandbox |

### Command provider contract

The command receives a JSON payload on stdin (or as configured) and must print a
JSON object on stdout. Prefer a dedicated worker binary over shell scripts.

Sandbox (see `docs/security.md`):

```text
RLM_PROVIDER_SANDBOX=strict|warn|off
RLM_PROVIDER_ALLOWED_DIRS=C:\tools\rlm-workers;D:\workers
```

### Adding a new provider (Rust)

1. Implement the provider interface under `src/rlm/provider/`.
2. Wire it in `src/rlm/provider/mod.rs` and the task runtime dispatch.
3. Add unit tests with a mock/dry-run path (no network).
4. Document env vars in `README.md` and security notes in `docs/security.md`.

## 2. Custom transform operations

Safe, non-executable transforms live in `src/rlm/transform.rs` and are exposed
via `rlm_transform` / CLI `transform --op`.

Supported ops today:

```text
dedupe_lines, sort_lines, filter_lines, head_lines, tail_lines,
truncate_chars, add_line_numbers, count_lines, normalize_whitespace
```

To add an op:

1. Extend the match in `apply(...)` with a pure function (no FS/network).
2. Document params in the op list returned by transform schema helpers.
3. Add a unit test in `transform.rs`.
4. Respect `RLM_MAX_TRANSFORM_BYTES`.

Do **not** use transforms for arbitrary code execution. That belongs to the
opt-in REPL backends (`docs/repl-execution-model.md`).

## 3. Hermes / release-gate plugins

The repo ships `plugins/hermes-evolution/` as a **separate** agent plugin
(skills + hooks), not an in-process rlm-mcp extension. Pattern:

- Ship skills under `plugins/*/skills/`
- Optional hooks under `plugins/*/hooks/`
- Document install for Codex/OpenCode separately from the MCP binary

## 4. What we intentionally do not support

- Runtime `dlopen` of untrusted `.so`/`.dll` plugins inside the MCP process
- Network calls without explicit opt-in env flags
- Silent shell execution for `RLM_PROVIDER_COMMAND` under `strict` sandbox

## 5. Verification checklist for extensions

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
# If tool schemas change:
cargo test write_tools_snapshot -- --ignored
cargo test tools_list_matches_snapshot
```
