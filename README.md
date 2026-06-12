# codebase-memory-rlm-mcp

Standalone **RLM (Recursive Language Model)** MCP server in **Rust**. Orchestrates map-reduce workflows by calling **codebase-memory-mcp** for graph operations.

**Not a fork of CBM** — runs alongside the graph server via MCP stdio client.

## Requires

- **[codebase-memory-mcp](https://github.com/stevenke1981/cbm-mcp)** installed and on PATH (or `CBM_BINARY` set)
- Indexed project (`index_repository` via CBM)

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

## MCP configuration (both servers)

```json
{
  "mcp": {
    "codebase-memory-mcp": {
      "type": "local",
      "command": ["codebase-memory-mcp"],
      "enabled": true
    },
    "codebase-memory-rlm-mcp": {
      "type": "local",
      "command": ["D:\\rlm-mcp\\target\\release\\codebase-memory-rlm-mcp.exe"],
      "enabled": true,
      "environment": {
        "CBM_PROJECT": "your-project-name",
        "CBM_BINARY": "D:\\cbm-mcp\\target\\release\\codebase-memory-mcp.exe"
      }
    }
  }
}
```

## Environment

| Variable | Purpose |
|----------|---------|
| `CBM_BINARY` | Path to `codebase-memory-mcp` executable |
| `CBM_COMMAND` | Space-separated launch command (alternative to `CBM_BINARY`) |
| `CBM_PROJECT` | Default project name (normalized to `cbm+` prefix) |
| `RLM_CACHE_DIR` | Session cache root (default: `%LOCALAPPDATA%\codebase-memory-rlm-mcp`) |

## RLM tools

`rlm_workflow`, `rlm_index_status`, `rlm_filter`, `rlm_read_symbol`, `rlm_trace`, `rlm_architecture`, `rlm_detect_changes`, `rlm_scan`, `rlm_chunk`, `rlm_peek`, `rlm_session_list`, `rlm_session_delete`

## Architecture

```
Agent → codebase-memory-rlm-mcp (RLM tools)
              ↓ MCP stdio per graph call
        codebase-memory-mcp (graph index)
```

Local scan sessions (`rlm_scan` / `rlm_chunk` / `rlm_peek`) persist under `RLM_CACHE_DIR/rlm-sessions`.

## Repo layout

| Repo | Responsibility |
|------|----------------|
| [cbm-mcp](https://github.com/stevenke1981/cbm-mcp) | Graph index + 14 CBM tools |
| [rlm-mcp](https://github.com/stevenke1981/rlm-mcp) | RLM workflow + chunk/peek (this repo) |