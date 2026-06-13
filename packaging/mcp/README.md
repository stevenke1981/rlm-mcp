# rlm-mcp MCP package

Handoff templates for **rlm-mcp** — standalone RLM server.

Server name: `rlm-mcp`  
Transport: stdio  
Binary: `rlm-mcp` or absolute path to release binary

Protocol negotiation, capabilities, JSON-RPC envelopes, and stdio transport
are provided by the official Rust MCP SDK (`rmcp 1.7.0`). Stdout is reserved
for MCP frames; logs go to stderr.

## Contract snapshot

`tools-list.snapshot.json` is the canonical `tools/list` contract (33 tools).

Human-readable schema: [`docs/tools.md`](../../docs/tools.md). Machine-readable: MCP `rlm_tools_reference` or `rlm-mcp tools-reference --json`.
CI compares live `tool_definitions()` against this file. Refresh after adding tools:

```bash
cargo test write_tools_snapshot -- --ignored
```

## Install

```powershell
.\install.ps1   # Windows
./install.sh    # Unix
```

The checkout installer downloads the latest GitHub Release binary, verifies checksums, copies the binary to `~/.config/rlm-mcp/bin/`, registers OpenCode and Codex, and installs the `rlm` skill. It does not compile Rust. Restart the agent or open a new session to load the tools. Use `.\install.ps1 -FromSource` or `./install.sh --from-source` only for local unreleased development.

Agents should follow `manifest.json` installer URLs or run `.\install.ps1`. The package manifest intentionally does not advertise `target/release` paths, because those are local build outputs and cause unnecessary compilation.

On Windows, if `rlm-mcp.exe` is locked by a running agent, the installer writes a side-by-side `rlm-mcp-<version>.exe` and configures OpenCode/Codex to launch that unlocked binary.

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
| `RLM_CACHE_DIR` | Session cache (default: OS cache dir / `rlm-mcp`) |

No `CBM_*` variables — this server does not call codebase-memory-mcp.

## Tools (33)

See `tools-list.snapshot.json` for the full contract. Core loop: `rlm_scan`, `rlm_peek`, `rlm_slice`, `rlm_transform`, `rlm_artifact_*`, `rlm_chunk`, `rlm_map_plan`, `rlm_map_claim`, `rlm_map_complete`, `rlm_reduce_merge`, `rlm_task_*`, `rlm_trajectory_*`, `rlm_budget_*`, `rlm_benchmark_*`.

## Optional: graph tools

For symbol-level code search, enable **codebase-memory-mcp** as a second MCP server (separate install). See [cbm-mcp dual-servers example](https://github.com/stevenke1981/cbm-mcp/blob/main/packaging/mcp/dual-servers.example.json).
