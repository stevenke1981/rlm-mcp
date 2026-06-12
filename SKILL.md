---
name: rlm
description: >
  Recursive Language Model for large/unstructured content. Standalone MCP server —
  external context via rlm_scan sessions, filter with rlm_peek, map with rlm_chunk.
  Triggers: analyze huge files, logs, long documents, scan all files, large repository,
  RLM, 10M+ token context, exploratory text analysis.
license: MIT
compatibility: opencode, codex, claude-code
metadata:
  mcp-server: codebase-memory-rlm-mcp
  paper: https://arxiv.org/pdf/2512.24601
---

# RLM (standalone)

**Context is external.** Never bulk-load files into root context. Use MCP tools only.

This skill uses **codebase-memory-rlm-mcp** alone — no graph index required.

## RLM loop

### Phase 1 — Load

```
rlm_workflow(phase="load")
rlm_scan(path=".")          # or path to log dir / file
```

Returns `session_id`, `chunk_count`, `total_bytes`, `files_scanned`.

### Phase 2 — Filter

```
rlm_peek(session_id, query="ERROR")
rlm_peek(session_id, query="auth")
```

Narrow to relevant paths/chunks before reading content.

### Phase 3 — Map (parallel)

```
rlm_chunk(session_id, file_pattern="app.log", offset=0, limit=3)
```

One sub-task per worker; each returns structured JSON findings.

### Phase 4 — Reduce

Merge worker outputs. Re-run filter→map only for proven gaps.

```
rlm_workflow(phase="reduce")
```

## Tool map

| Task | Tool |
|------|------|
| Workflow help | `rlm_workflow` |
| Load context | `rlm_scan` |
| Filter/search | `rlm_peek` |
| Read chunks | `rlm_chunk` |
| List sessions | `rlm_session_list` |
| Delete session | `rlm_session_delete` |

## Rules

1. Never load 10+ files into root context
2. `rlm_scan` once per analysis scope; reuse `session_id`
3. Filter (`rlm_peek`) before large `rlm_chunk` reads
4. Keep `limit` small (3–5 chunks per call)
5. Reduce to structured JSON before final natural-language answer

## Optional: graph tools

If the agent also has **codebase-memory-mcp** enabled, use graph tools directly for symbol-level code search. That is a **separate MCP server** — not part of this RLM skill.