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
  mcp-server: rlm-mcp
  paper: https://arxiv.org/pdf/2512.24601
---

# RLM (standalone)

**Context is external.** Never bulk-load files into root context. Use MCP tools only.

This skill uses **rlm-mcp** alone — no graph index required.

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
| Inspect session | `rlm_env_info` |
| Filter/search | `rlm_peek` |
| Slice/transform | `rlm_slice`, `rlm_transform` |
| Artifacts | `rlm_artifact_write`, `rlm_artifact_read` |
| Read chunks | `rlm_chunk` |
| Parallel map | `rlm_map_plan`, `rlm_map_claim`, `rlm_map_complete` |
| Reduce | `rlm_reduce_schema`, `rlm_reduce_merge` |
| Recursive tasks | `rlm_task_create`, `rlm_task_list`, `rlm_task_result`, `rlm_task_reduce`, `rlm_task_cancel` |
| Budget/trajectory | `rlm_budget_configure`, `rlm_budget_status`, `rlm_trajectory_get`, `rlm_trajectory_final` |
| List sessions | `rlm_session_list` |
| Session maintenance | `rlm_session_delete`, `rlm_session_cleanup`, `rlm_session_export`, `rlm_session_import` |
| Benchmarks/reference | `rlm_benchmark_list`, `rlm_benchmark_run`, `rlm_tools_reference` |

## Rules

1. Never load 10+ files into root context
2. `rlm_scan` once per analysis scope; reuse `session_id`
3. Filter (`rlm_peek`) before large `rlm_chunk` reads
4. Keep `limit` small (3–5 chunks per call)
5. Reduce to structured JSON before final natural-language answer

## Optional: graph tools

If the agent also has **codebase-memory-mcp** enabled, use graph tools directly for symbol-level code search. That is a **separate MCP server** — not part of this RLM skill.
