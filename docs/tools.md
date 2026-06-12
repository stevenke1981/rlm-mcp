# rlm-mcp — MCP tool reference

Machine-readable source of truth: call `rlm_tools_reference` (MCP) or `rlm-mcp tools-reference --json` (CLI).

Contract snapshot: [`packaging/mcp/tools-list.snapshot.json`](../packaging/mcp/tools-list.snapshot.json) (33 tools).

## Conventions

- **MCP arguments** use `snake_case` (e.g. `session_id`, `chunk_ids`).
- **CLI flags** use `kebab-case` (e.g. `--session-id`, `--chunk-id`). Repeat flags for arrays.
- Append **`--json`** or **`--quiet`** on CLI for parseable stdout.
- Offline tests use providers `mock` and `dry-run` only.
- Live providers: `command` (local executable) and `openai` (requires `RLM_ALLOW_NETWORK=1` + `RLM_OPENAI_API_KEY`).

## Load phase

### `rlm_scan`

Load a filesystem path or inline text into an external session.

| MCP argument | Type | CLI flag | Notes |
|--------------|------|----------|-------|
| `path` | string | `--path` | File or directory |
| `content` | string | `--content` | Inline text (alternative to path) |
| `virtual_path` | string | `--virtual-path` | Filename label for content (default `inline.txt`) |
| `variable_name` | string | `--variable` | Optional named variable |

**Returns:** `session_id`, `chunk_count`, `total_bytes`, skip metadata.

```bash
rlm-mcp scan --content "log line" --virtual-path app.log --json
```

### `rlm_env_info`

Inspect session as external environment.

| MCP argument | Required | CLI flag |
|--------------|----------|----------|
| `session_id` | yes | `--session-id` |

**Returns:** `files`, `chunk_ids`, `context_length_bytes`, expiry timestamps.

### `rlm_session_list`

List active persisted sessions. No arguments.

### `rlm_session_delete`

| MCP argument | Required | CLI flag |
|--------------|----------|----------|
| `session_id` | yes | `--session-id` |

### `rlm_session_cleanup`

Remove expired sessions from disk. Safe for concurrent readers (tombstone + atomic files).

### `rlm_session_export`

| MCP argument | Required | CLI flag |
|--------------|----------|----------|
| `session_id` | yes | `--session-id` |

Returns full `session` object for backup/transfer.

### `rlm_session_import`

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `session` | `--session-json` / `--stdin` | required |
| `preserve_id` | `--preserve-id` | false |

## Filter phase

### `rlm_peek`

Search and filter without loading full context into the model.

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `session_id` | `--session-id` | required |
| `query` | `--query` | substring or regex pattern |
| `path_filter` | `--path` | |
| `glob` | `--glob` | e.g. `*.log` |
| `regex` | `--regex` | false |
| `bm25` | `--bm25` | false (token BM25 ranking; requires `query`) |
| `case_sensitive` | `--ignore-case` (inverted) | true |
| `line_start` / `line_end` | `--line-start` / `--line-end` | |
| `context_radius` | `--context` | 2 |
| `limit` | `--limit` | 20 |
| `include_content` | `--full` | false |

**Returns:** `matches[]` with `chunk_id`, `line`, `preview` (and `bm25_score` when `bm25=true`); `search_mode` is `substring`, `regex`, or `bm25`.

### `rlm_slice`

Read a line range from one chunk (REPL-style slice).

| MCP argument | CLI flag |
|--------------|----------|
| `session_id` | `--session-id` |
| `chunk_id` | `--chunk-id` |
| `start_line` / `end_line` | `--start` / `--end` |

### `rlm_transform`

Safe deterministic text transforms (no code execution). See [`docs/repl-execution-model.md`](repl-execution-model.md).

| MCP argument | CLI flag | Notes |
|--------------|----------|-------|
| `session_id` | `--session-id` | required |
| `operation` | `--op` | required |
| `params` | `--params` | JSON object |
| `chunk_id` | `--chunk-id` | input source |
| `artifact_name` | `--artifact` | input source |
| `content` | `--content` | inline input |

### `rlm_artifact_write`

| MCP argument | CLI flag |
|--------------|----------|
| `session_id` | `--session-id` |
| `name` | `--name` |
| `content` | `--content` |
| `source_chunk_id` | `--chunk-id` |

### `rlm_artifact_read`

| MCP argument | CLI flag |
|--------------|----------|
| `session_id` | `--session-id` |
| `name` | `--name` |
| `start_line` / `end_line` | `--start` / `--end` |

## Map phase

### `rlm_chunk`

Paginated chunk reads for map workers.

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `session_id` | `--session-id` | required |
| `file_pattern` | `--file-pattern` | |
| `chunk_ids` | `--chunk-id` (repeatable) | |
| `offset` | `--offset` | 0 |
| `limit` | `--limit` | 5 |
| `include_metadata` | `--metadata` | true |

**Returns:** each chunk includes `content`, `truncated`, `max_chunk_bytes`. Response may set `any_truncated: true`. Limit via `RLM_MAX_CHUNK_BYTES` (default 256 KiB).

### `rlm_map_plan`

Create parallel work batches from chunk IDs or file pattern.

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `session_id` | `--session-id` | required |
| `chunk_ids` | `--chunk-id` | |
| `file_pattern` | `--file-pattern` | |
| `batch_size` | `--batch-size` | 3 |

**Returns:** `plan_id`, `batches`, `worker_output_schema`. Plan is persisted for claim/complete.

### `rlm_map_claim`

Claim the next pending batch (or a specific `batch_id`) for a worker.

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `plan_id` | `--plan-id` | required |
| `worker_id` | `--worker-id` | required |
| `batch_id` | `--batch-id` | optional (next pending if omitted) |

### `rlm_map_complete`

Mark a claimed batch complete and store worker JSON output.

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `plan_id` | `--plan-id` | required |
| `worker_id` | `--worker-id` | required |
| `batch_id` | `--batch-id` | required |
| `output` | `--output` | required (JSON string on CLI) |

## Reduce phase

### `rlm_reduce_schema`

Returns worker JSON schema, final-answer schema, and reduce checklist. No arguments.

### `rlm_reduce_merge`

| MCP argument | CLI flag |
|--------------|----------|
| `worker_outputs` | `--workers` (JSON array string) |

**Returns:** merged `findings`, `coverage`, `needs_recursion`.

## Recurse phase

### `rlm_task_create`

| MCP argument | CLI flag | Notes |
|--------------|----------|-------|
| `session_id` | `--session-id` | required |
| `prompt` | `--prompt` | required |
| `chunk_ids` | `--chunk-id` | repeatable |
| `parent_task_id` | `--parent-task-id` | for child tasks |
| `provider` | `--provider` | `mock` (default), `dry-run`, `command`, `openai` |
| `execute` | `--no-execute` (inverted) | default true |
| `budget` | — | MCP object: max_depth, fanout, subcalls, bytes, wall time |

### `rlm_task_list`

Optional filters: `session_id` (`--session-id`), `root_id` (`--root-id`).

### `rlm_task_result`

| MCP argument | CLI flag |
|--------------|----------|
| `task_id` | `--task-id` |

### `rlm_task_reduce`

| MCP argument | CLI flag |
|--------------|----------|
| `root_id` | `--root-id` |

### `rlm_task_cancel`

| MCP argument | CLI flag |
|--------------|----------|
| `root_id` | `--root-id` |
| `reason` | `--reason` |

## Observe phase

### `rlm_trajectory_get`

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `session_id` | `--session-id` | required |
| `format` | `--format` | `json` (`jsonl`, `replay`) |
| `redact` | `--no-redact` (inverted) | true |
| `redact_patterns` | `--redact-pattern` | repeatable |

### `rlm_trajectory_final`

| MCP argument | CLI flag |
|--------------|----------|
| `session_id` | `--session-id` |
| `answer` | `--answer` |
| `evidence_count` | `--evidence-count` |

### `rlm_budget_status`

| MCP argument | CLI flag |
|--------------|----------|
| `session_id` | `--session-id` |

## Control phase

### `rlm_budget_configure`

| MCP argument | CLI flag |
|--------------|----------|
| `session_id` | `--session-id` |
| `mode` | `--soft-warning` for soft_warning, else fail_fast |
| `max_chunks_read` | `--max-chunks` |
| `max_sub_calls` | `--max-sub-calls` |
| `max_total_tokens_est` | `--max-tokens` |
| `max_wall_secs` | `--max-wall-secs` |

### `rlm_repl_info`

Lists REPL sandbox backends, capability flags, and limits. CLI: `rlm-mcp repl-info --json`.

### `rlm_repl_execute`

Opt-in code execution (`RLM_ALLOW_REPL_EXEC=1`). CLI: `rlm-mcp repl-exec --session-id <id> --code "..." --backend command --json`.

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `session_id` | `--session-id` | required |
| `code` | `--code` | required |
| `language` | `--language` | `text` |
| `backend` | `--backend` | `command` |

## Benchmark

### `rlm_benchmark_list`

Lists offline suites (S-NIAH `mini`/`small`/`large`/`nightly`). CLI: `rlm-mcp benchmark list --json`.

### `rlm_benchmark_run`

| MCP argument | CLI | Default |
|--------------|-----|---------|
| `suite` | `benchmark sniah` | `sniah` |
| `fixture_size` | `--size` | `mini` |

## Help

### `rlm_workflow`

| MCP argument | CLI flag | Default |
|--------------|----------|---------|
| `phase` | `--phase` | `overview` (`load`, `filter`, `map`, `reduce`) |

### `rlm_tools_reference`

Returns this reference as structured JSON for agents. CLI: `rlm-mcp tools-reference --json`.

## Typical loop

1. `rlm_scan` → get `session_id`
2. `rlm_peek` → narrow to `chunk_id` values
3. `rlm_map_plan` + `rlm_map_claim` + `rlm_chunk` + `rlm_map_complete` → worker batches
4. Workers produce JSON → `rlm_reduce_merge`
5. If `needs_recursion`: `rlm_task_create` → `rlm_task_reduce`
6. `rlm_trajectory_final` + `rlm_budget_status` for cost/coverage audit