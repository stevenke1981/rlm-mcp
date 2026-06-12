# codebase-memory-rlm-mcp MCP package

Handoff templates for **codebase-memory-rlm-mcp** — standalone RLM server.

Server name: `codebase-memory-rlm-mcp`  
Transport: stdio  
Binary: `codebase-memory-rlm-mcp` or absolute path to release binary

## Build

```powershell
cargo build --release
```

Windows: `target\release\codebase-memory-rlm-mcp.exe`  
Unix: `target/release/codebase-memory-rlm-mcp`

## Install

```powershell
.\install.ps1   # Windows
./install.sh    # Unix
```

Copies binary to `~/.config/codebase-memory-rlm-mcp/bin/` and installs the `rlm` skill.

## Manual config

| Template | Target |
|----------|--------|
| `generic-mcp.json` | Claude-style `mcpServers` |
| `codex-config.toml` | Codex `config.toml` |
| `opencode.json` | OpenCode `opencode.json` |
| `claude-settings.json` | Claude Code settings |
| `manifest.json` | Package summary |

Replace `{{RLM_BINARY}}` with an absolute path.

## Environment

| Variable | Purpose |
|----------|---------|
| `RLM_CACHE_DIR` | Session cache (default: OS cache dir / `codebase-memory-rlm-mcp`) |

No `CBM_*` variables — this server does not call codebase-memory-mcp.

## Tools (6)

`rlm_workflow`, `rlm_scan`, `rlm_peek`, `rlm_chunk`, `rlm_session_list`, `rlm_session_delete`

## Optional: graph tools

For symbol-level code search, enable **codebase-memory-mcp** as a second MCP server (separate install). See [cbm-mcp dual-servers example](https://github.com/stevenke1981/cbm-mcp/blob/main/packaging/mcp/dual-servers.example.json).