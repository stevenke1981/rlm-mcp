# codebase-memory-rlm-mcp

Standalone **RLM (Recursive Language Model)** MCP server. Integrates map-reduce orchestration with **codebase-memory-mcp** graph tools.

**Not a fork of CBM** — runs alongside the graph server via MCP client.

## Requires

- **[codebase-memory-mcp](D:\cbm-mcp)** installed and on PATH (or `CBM_BINARY` set)
- Indexed project (`index_repository` via CBM)

## Install

```powershell
cd D:\rlm-mcp
pip install -e .
```

Windows:

```powershell
.\install.ps1
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
      "command": ["python", "-m", "codebase_memory_rlm_mcp"],
      "enabled": true,
      "environment": {
        "CBM_PROJECT": "your-project-name",
        "CBM_BINARY": "D:\\cbm-mcp\\target\\release\\codebase-memory-mcp.exe"
      }
    }
  }
}
```

## RLM tools

`rlm_workflow`, `rlm_index_status`, `rlm_filter`, `rlm_read_symbol`, `rlm_trace`, `rlm_detect_changes`, `rlm_scan`, `rlm_chunk`, `rlm_peek`, `rlm_session_list`, `rlm_session_delete`

## Repo layout

| Repo | Responsibility |
|------|----------------|
| `D:\cbm-mcp` | Graph index + 14 CBM tools |
| `D:\rlm-mcp` | RLM workflow + chunk/peek (this repo) |