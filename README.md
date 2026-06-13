# rlm-mcp

Standalone **RLM (Recursive Language Model)** MCP server in **Rust**.

Implements the [MIT CSAIL paper](https://arxiv.org/pdf/2512.24601) pattern: **context is external** — load long text into sessions, filter with peek, map with paginated chunks, reduce in the agent.

**Independent project** — no dependency on codebase-memory-mcp or any graph index.

## When to use which MCP

| Need | Use |
|------|-----|
| Long logs, docs, transcripts, multi-file text analysis | **rlm-mcp** (this repo) |
| Code graph, symbol lookup, call-path tracing | **cbm-mcp** (separate repo) |
| Both long-context sessions and graph search | Run both MCP servers side by side — separate processes, no code coupling |

## Quick start

```powershell
cd D:\rlm-mcp
.\install.ps1
rlm-mcp --version
```

`install.ps1` / `install.sh` download the latest GitHub Release binary by default. Agents can install directly from a checkout without compiling Rust.

For agents and automation: run `.\install.ps1` from the checkout, or run the raw release installer URL below. Do not compile or use `target/release` unless you explicitly pass `-FromSource`.

## Install

### From GitHub Release

Windows:

```powershell
irm https://raw.githubusercontent.com/stevenke1981/rlm-mcp/main/packaging/windows/install.ps1 | iex
```

Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/stevenke1981/rlm-mcp/main/packaging/linux/install.sh | bash
```

macOS Apple Silicon:

```bash
curl -fsSL https://raw.githubusercontent.com/stevenke1981/rlm-mcp/main/packaging/macos/install.sh | bash
```

The release installer verifies `SHA256SUMS.txt`, installs the binary to a stable path, and registers both OpenCode and Codex MCP entries automatically.
On Windows, if the stable binary is currently locked by a running agent, the installer installs a versioned side-by-side binary and configures agents to use that path.

### From checkout without compiling

### Windows

```powershell
.\install.ps1
```

Downloads the latest release archive, verifies `SHA256SUMS.txt`, installs to `%USERPROFILE%\.config\rlm-mcp\bin\rlm-mcp.exe`, registers OpenCode and Codex, and copies the `rlm` skill for Codex, Claude Code, OpenCode, and agents.

### Linux / macOS

```bash
chmod +x install.sh
./install.sh
```

Installs to `~/.config/rlm-mcp/bin/rlm-mcp` and symlinks `~/.local/bin/rlm-mcp`.

Pin a version:

```powershell
.\install.ps1 -Version v0.1.6
```

```bash
RLM_VERSION=v0.1.6 ./install.sh
```

### Build from source checkout

Only use this for development or local unreleased changes:

```powershell
.\install.ps1 -FromSource
```

```bash
./install.sh --from-source
```

### MCP configuration

Templates: [`packaging/mcp/`](packaging/mcp/) (OpenCode, Codex, Claude, generic).

```json
{
  "rlm-mcp": {
    "type": "local",
    "command": ["rlm-mcp"],
    "enabled": true,
    "timeout": 120000
  }
}
```

Replace `command` with the absolute path from `install.ps1` / `install.sh`, or use `{{RLM_BINARY}}` in templates.

The installer updates both existing `opencode.json` and `opencode.jsonc` files and `%USERPROFILE%\.codex\config.toml`. Restart the agent or open a new session after installation because MCP tools are loaded when a session starts.

The MCP protocol boundary uses the official Rust SDK, `rmcp 1.7.0`, for stdio
framing, protocol negotiation, capabilities, request IDs, and error envelopes.
The 33-tool contract remains locked by
[`packaging/mcp/tools-list.snapshot.json`](packaging/mcp/tools-list.snapshot.json).
Stdout is protocol-only in MCP mode; diagnostics are written to stderr.

**Stable binary path:**

| OS | Path |
|----|------|
| Windows | `%USERPROFILE%\.config\rlm-mcp\bin\rlm-mcp.exe` |
| Linux / macOS | `~/.config/rlm-mcp/bin/rlm-mcp` (symlinked from `~/.local/bin/rlm-mcp`) |

### Releases

Push a version tag to trigger the GitHub release workflow:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Artifacts: per-target `.zip` / `.tar.gz` + `SHA256SUMS.txt`. Local packaging:

```powershell
cargo build --release
.\scripts\package-release.ps1
```

```bash
cargo build --release
./scripts/package-release.sh
```

## Environment

| Variable | Default | Purpose |
|----------|---------|---------|
| `RLM_CACHE_DIR` | `%LOCALAPPDATA%\rlm-mcp` / `~/.cache/...` | Dedicated cache root (auto-creates `rlm-sessions`, `rlm-artifacts`, …) |
| `RLM_ALLOW_SYSTEM_TEMP` | unset | Set `1` only if you must use bare OS temp as cache |
| `RLM_MAX_FILE_BYTES` | `524288` | Max single file size |
| `RLM_MAX_TOTAL_BYTES` | `8388608` | Max total session bytes |
| `RLM_MAX_CHUNKS` | `10000` | Max chunks per session |
| `RLM_MAX_SESSIONS` | `50` | Max persisted sessions |
| `RLM_SESSION_TTL_SECS` | `3600` | Session expiry |
| `RLM_CHUNK_LINES` | `200` | Lines per chunk |
| `RLM_ALLOW_NETWORK` | unset (offline) | Set to `1` to enable `openai` provider |
| `RLM_OPENAI_API_KEY` | — | OpenAI-compatible API key (never persisted) |
| `RLM_OPENAI_BASE_URL` | `https://api.openai.com/v1` | Compatible API base URL |
| `RLM_OPENAI_MODEL` | `gpt-4o-mini` | Model name for sub-calls |
| `RLM_PROVIDER_COMMAND` | — | Executable for `command` provider |
| `RLM_PROVIDER_ARGS` | `[]` | JSON array or whitespace-separated args |
| `RLM_PROVIDER_MAX_RETRIES` | `3` | Provider retry attempts |
| `RLM_OPENAI_PROMPT_COST_PER_1K` | — | Optional USD/1K prompt tokens for cost est. |
| `RLM_OPENAI_COMPLETION_COST_PER_1K` | — | Optional USD/1K completion tokens |

## RLM loop

| Phase | Tools | Purpose |
|-------|-------|---------|
| Load | `rlm_scan`, `rlm_env_info` | Load path or text into session; inspect metadata |
| Filter | `rlm_peek`, `rlm_slice` | Narrow candidates (substring, glob, regex, line range) |
| REPL | `rlm_env_info`, `rlm_slice`, `rlm_transform`, `rlm_artifact_*` | Safe snippet transforms and derived artifacts |
| Map | `rlm_chunk`, `rlm_map_plan`, `rlm_map_claim`, `rlm_map_complete` | Paginated chunks; coordinated parallel workers |
| Reduce | `rlm_reduce_schema`, `rlm_reduce_merge` | Merge worker JSON; decide if recursion needed |
| Recurse | `rlm_task_create`, `rlm_task_list`, `rlm_task_result`, `rlm_task_reduce` | Sub-tasks (`mock`/`dry-run` offline; `command`/`openai` opt-in) |
| Observe | `rlm_trajectory_get`, `rlm_trajectory_final`, `rlm_budget_status` | Trajectory + budget/tail-cost reporting |
| Control | `rlm_budget_configure`, `rlm_task_cancel` | Session limits, fail-fast/soft-warning, cancel trees |
| Help | `rlm_workflow`, `rlm_tools_reference` | Phase guidance + full tool schema |

Also: `rlm_session_list`, `rlm_session_delete`, `rlm_benchmark_list`, `rlm_benchmark_run`

Full parameter reference: [`docs/tools.md`](docs/tools.md)

Walkthrough and examples: [`docs/rlm-loop.md`](docs/rlm-loop.md), [`examples/`](examples/)

Paper ↔ implementation map: [`docs/paper-mapping.md`](docs/paper-mapping.md)

Limitations and benchmarks: [`docs/limitations.md`](docs/limitations.md), [`docs/benchmarks.md`](docs/benchmarks.md)

## CLI (non-MCP)

Run without args to start MCP stdio server. With a subcommand, outputs JSON:

```powershell
# Load directory
rlm-mcp scan --path . --json

# Load inline text
rlm-mcp scan --content "long prompt text" --virtual-path prompt.txt --json

# Filter
rlm-mcp peek --session-id <id> --query ERROR --limit 10 --json

# Map
rlm-mcp chunk --session-id <id> --chunk-id c-0 --json
rlm-mcp map-plan --session-id <id> --batch-size 3 --json

# Reduce
rlm-mcp reduce-schema --json
rlm-mcp reduce-merge --workers '[{"batch_id":"b0","findings":[]}]' --json
```

## Architecture

```
Agent (LLM plans filter/map/reduce)
    ↓ MCP stdio or CLI
rlm-mcp
    ↓ local sessions (RLM_CACHE_DIR/rlm-sessions)
External files / logs / docs / text blobs
```

`rmcp` implements the MCP transport boundary. The external-context, filter,
map, reduce, recursion, provider, budget, and trajectory behavior remains this
project's RLM implementation.

## Related projects

| Repo | Role |
|------|------|
| [rlm-mcp](https://github.com/stevenke1981/rlm-mcp) | **This repo** — standalone RLM |
| [cbm-mcp](https://github.com/stevenke1981/cbm-mcp) | Optional separate graph MCP (not required) |

## Install troubleshooting

| Symptom | Fix |
|---------|-----|
| `rlm-mcp` not found in agent | Use absolute path from install output; avoid bare `rlm-mcp` unless `~/.local/bin` is on `PATH` |
| OpenCode shows no `rlm-mcp` connection after install | Re-run `.\install.ps1`; then restart OpenCode or open a new session because MCP servers are loaded at session start |
| MCP server exits immediately | Run without subcommand for stdio MCP; use `rlm-mcp workflow --json` to verify CLI |
| `tools/list` test fails after adding tools | `cargo test write_tools_snapshot -- --ignored` then commit snapshot |
| Session not found across processes | Same `RLM_CACHE_DIR`; use `rlm_session_list --json` |
| Permission denied on cache dir | Set `RLM_CACHE_DIR` to a writable directory |
| Windows build slow | Default install does not build; use `.\install.ps1` for release binary or `.\install.ps1 -FromSource -SkipBuild` after a local dev build |
| Windows says `rlm-mcp.exe` is being used by another process | Re-run the current installer; it falls back to `%USERPROFILE%\.config\rlm-mcp\bin\rlm-mcp-<version>.exe` and updates agent config |
| Release smoke skipped | Run `cargo build --release` then `cargo test --test release_smoke --release` |

## Implementation roadmap

See [`TODO.md`](TODO.md) for the full paper-complete implementation backlog.

**Current status:** Paper-complete core (P0–P3); optional benchmark adapters: S-NIAH + OOLONG shipped; BrowseComp/CodeQA planned.
