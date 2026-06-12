---
name: rlm
description: >
  Recursive Language Model for large codebases, integrated with codebase-memory-mcp.
  Use MCP tools rlm_filter, rlm_read_symbol, rlm_trace for graph-native map-reduce;
  rlm_scan/rlm_chunk for logs and huge files. Triggers: analyze codebase, scan all files,
  large repository, RLM, find usage across project, security audit at scale.
license: MIT
compatibility: opencode, codex, claude-code
metadata:
  mcp-server: codebase-memory-rlm-mcp
  requires: codebase-memory-mcp
  paper: https://arxiv.org/pdf/2512.24601
---

# RLM + codebase-memory-mcp

**Context is external.** Use MCP tools — never bulk-read the repo into main context.

## Prerequisites

1. `codebase-memory-mcp` installed and running
2. `codebase-memory-rlm-mcp` MCP server enabled
3. Project indexed (`index_repository` via codebase-memory-mcp)

Set `CBM_PROJECT` env var or pass `project` on every graph tool call.

## RLM loop (graph-native)

### Phase 1 — Filter

```
rlm_workflow(phase="filter")
rlm_index_status(project)
rlm_filter(project, query="auth middleware", label="Function")
rlm_filter(project, pattern="UserID")          # files mode
```

For logs/CSV (not in graph): `rlm_scan(path)` → `rlm_peek(session_id, query)`

### Phase 2 — Map (parallel)

One tool call per worker — never combine symbols:

```
rlm_read_symbol(project, qualified_name="api.routes.createUser")
rlm_trace(project, function_name="handleAuth", mode="calls")
rlm_chunk(session_id, offset=0, limit=3)       # huge files only
```

Spawn 3–10 parallel sub-agents; each handles 1 symbol or 1 chunk.

### Phase 3 — Reduce

Merge worker JSON. Fill gaps with `rlm_trace` or `rlm_detect_changes`.

```
rlm_workflow(phase="reduce")
rlm_detect_changes(project)
```

## Tool map

| Task | MCP tool |
|------|----------|
| Check index | `rlm_index_status` |
| Filter symbols | `rlm_filter(query/label)` |
| Filter file paths | `rlm_filter(pattern)` |
| Read one symbol | `rlm_read_symbol` |
| Trace calls/impact | `rlm_trace` |
| Architecture | `rlm_architecture` |
| Git impact | `rlm_detect_changes` |
| Scan logs/CSV | `rlm_scan` |
| Peek in session | `rlm_peek` |
| Chunk huge file | `rlm_chunk` |
| Workflow help | `rlm_workflow` |

## Rules

1. Never load 10+ files into root context
2. `rlm_read_symbol` = one qualified_name per call
3. Prefer graph tools over `rg` when project is indexed
4. Use `rlm_scan`/`rlm_chunk` only for non-code or unindexed blobs
5. Always reduce to structured JSON before final answer