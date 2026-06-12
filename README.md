# codebase-memory-rlm-mcp

Standalone **RLM (Recursive Language Model)** MCP server in **Rust**.

Implements the [MIT CSAIL paper](https://arxiv.org/pdf/2512.24601) pattern: **context is external** — load long text into sessions, filter with peek, map with paginated chunks, reduce in the agent.

**Independent project** — no dependency on codebase-memory-mcp or any graph index.

## When to use which MCP

| Need | Use |
|------|-----|
| Long logs, docs, transcripts, multi-file text analysis | **rlm-mcp** (this repo) |
| Code graph, symbol lookup, call-path tracing | **cbm-mcp** (separate repo) |
| Both long-context sessions and graph search | Run both MCP servers side by side — separate processes, no code coupling |

## Build

```powershell
cd D:\rlm-mcp
cargo build --release
```

Binary: `target\release\codebase-memory-rlm-mcp.exe` (Windows) or `target/release/codebase-memory-rlm-mcp` (Unix).

## Install

### Windows

```powershell
.\install.ps1
```

Installs to `%USERPROFILE%\.config\codebase-memory-rlm-mcp\bin\codebase-memory-rlm-mcp.exe`

### Linux / macOS

```bash
chmod +x install.sh
./install.sh
```

Installs to `~/.config/codebase-memory-rlm-mcp/bin/codebase-memory-rlm-mcp` and symlinks `~/.local/bin/codebase-memory-rlm-mcp`.

### MCP configuration

Templates: [`packaging/mcp/`](packaging/mcp/) (OpenCode, Codex, Claude, generic).

```json
{
  "codebase-memory-rlm-mcp": {
    "type": "local",
    "command": ["codebase-memory-rlm-mcp"],
    "enabled": true,
    "timeout": 120000
  }
}
```

Replace `command` with the absolute path from `install.ps1` / `install.sh`, or use `{{RLM_BINARY}}` in templates.

## Environment

| Variable | Default | Purpose |
|----------|---------|---------|
| `RLM_CACHE_DIR` | `%LOCALAPPDATA%\codebase-memory-rlm-mcp` / `~/.cache/...` | Session cache root |
| `RLM_MAX_FILE_BYTES` | `524288` | Max single file size |
| `RLM_MAX_TOTAL_BYTES` | `8388608` | Max total session bytes |
| `RLM_MAX_CHUNKS` | `10000` | Max chunks per session |
| `RLM_MAX_SESSIONS` | `50` | Max persisted sessions |
| `RLM_SESSION_TTL_SECS` | `3600` | Session expiry |
| `RLM_CHUNK_LINES` | `200` | Lines per chunk |

## RLM loop

| Phase | Tools | Purpose |
|-------|-------|---------|
| Load | `rlm_scan`, `rlm_env_info` | Load path or text into session; inspect metadata |
| Filter | `rlm_peek`, `rlm_slice` | Narrow candidates (substring, glob, regex, line range) |
| Map | `rlm_chunk`, `rlm_map_plan` | Paginated chunk reads; parallel work batches |
| Reduce | `rlm_reduce_schema`, `rlm_reduce_merge` | Merge worker JSON; decide if recursion needed |
| Recurse | `rlm_task_create`, `rlm_task_list`, `rlm_task_result`, `rlm_task_reduce` | Sub-tasks with mock/dry-run provider |
| Observe | `rlm_trajectory_get`, `rlm_trajectory_final`, `rlm_budget_status` | Trajectory + budget/tail-cost reporting |
| Control | `rlm_budget_configure`, `rlm_task_cancel` | Session limits, fail-fast/soft-warning, cancel trees |
| Help | `rlm_workflow` | Phase guidance |

Also: `rlm_session_list`, `rlm_session_delete`

## CLI (non-MCP)

Run without args to start MCP stdio server. With a subcommand, outputs JSON:

```powershell
# Load directory
codebase-memory-rlm-mcp scan --path . --json

# Load inline text
codebase-memory-rlm-mcp scan --content "long prompt text" --virtual-path prompt.txt --json

# Filter
codebase-memory-rlm-mcp peek --session-id <id> --query ERROR --limit 10 --json

# Map
codebase-memory-rlm-mcp chunk --session-id <id> --chunk-id c-0 --json
codebase-memory-rlm-mcp map-plan --session-id <id> --batch-size 3 --json

# Reduce
codebase-memory-rlm-mcp reduce-schema --json
codebase-memory-rlm-mcp reduce-merge --workers '[{"batch_id":"b0","findings":[]}]' --json
```

## Architecture

```
Agent (LLM plans filter/map/reduce)
    ↓ MCP stdio or CLI
codebase-memory-rlm-mcp
    ↓ local sessions (RLM_CACHE_DIR/rlm-sessions)
External files / logs / docs / text blobs
```

## Related projects

| Repo | Role |
|------|------|
| [rlm-mcp](https://github.com/stevenke1981/rlm-mcp) | **This repo** — standalone RLM |
| [cbm-mcp](https://github.com/stevenke1981/cbm-mcp) | Optional separate graph MCP (not required) |

## Implementation roadmap

See [`TODO.md`](TODO.md) for the full paper-complete implementation backlog.

**Current status:** P0 complete; P1 recursive sub-call + trajectory logging (JSONL/replay/redaction) with per-run cost summary.