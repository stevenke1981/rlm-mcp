# codebase-memory-rlm-mcp

Standalone **RLM (Recursive Language Model)** MCP server in **Rust**.

Implements the [MIT CSAIL paper](https://arxiv.org/pdf/2512.24601) pattern: **context is external** — load long text into sessions, filter with peek, map with paginated chunks, reduce in the agent.

**Independent project** — no dependency on codebase-memory-mcp or any graph index.

## Build

```powershell
cd D:\rlm-mcp
cargo build --release
```

Binary: `target\release\codebase-memory-rlm-mcp.exe` (Windows) or `target/release/codebase-memory-rlm-mcp` (Unix).

## Install

Windows:

```powershell
.\install.ps1
```

Unix:

```bash
./install.sh
```

## MCP configuration

```json
{
  "codebase-memory-rlm-mcp": {
    "type": "local",
    "command": ["D:\\rlm-mcp\\target\\release\\codebase-memory-rlm-mcp.exe"],
    "enabled": true
  }
}
```

## Environment

| Variable | Purpose |
|----------|---------|
| `RLM_CACHE_DIR` | Session cache root (default: `%LOCALAPPDATA%\codebase-memory-rlm-mcp`) |

## RLM loop

| Phase | Tool | Purpose |
|-------|------|---------|
| Load | `rlm_scan` | Load path into session; get metadata |
| Filter | `rlm_peek` | Substring/path search without full read |
| Map | `rlm_chunk` | Paginated chunk reads (parallel workers) |
| Reduce | (agent) | Merge worker JSON into final answer |
| Help | `rlm_workflow` | Phase guidance |

Also: `rlm_session_list`, `rlm_session_delete`

## Architecture

```
Agent (LLM plans filter/map/reduce)
    ↓ MCP stdio
codebase-memory-rlm-mcp
    ↓ local sessions (RLM_CACHE_DIR/rlm-sessions)
External files / logs / docs
```

## Related projects

| Repo | Role |
|------|------|
| [rlm-mcp](https://github.com/stevenke1981/rlm-mcp) | **This repo** — standalone RLM |
| [cbm-mcp](https://github.com/stevenke1981/cbm-mcp) | Optional separate graph MCP (not required) |

Use both MCP servers in the same agent only if you want graph search **and** RLM sessions — they are separate processes with no code coupling.