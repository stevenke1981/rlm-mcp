# Parallel map workers (claim / complete)

Parent agent creates a plan; worker agents claim batches without duplicating work.

## 1. Load context

```bash
rlm-mcp scan --content "$(Get-Content app.log -Raw)" --virtual-path app.log --json
```

Note `session_id` from the response.

## 2. Filter (optional)

```bash
rlm-mcp peek --session-id <session_id> --query ERROR --json
```

## 3. Plan batches

```bash
rlm-mcp map-plan --session-id <session_id> --batch-size 2 --json
```

Response includes `plan_id` and `batches` with stable `batch_id` values. The plan is persisted under `RLM_CACHE_DIR/rlm-map-plans/`.

## 4. Worker claims a batch

Each worker uses a unique `worker_id`:

```bash
rlm-mcp map-claim --plan-id <plan_id> --worker-id worker-a --json
```

Returns `batch_id`, `chunk_ids`, and `remaining_pending`. Status `no_work` means all batches are claimed or done.

## 5. Worker reads chunks

```bash
rlm-mcp chunk --session-id <session_id> --chunk-id <chunk_id> --json
```

## 6. Worker completes batch

Submit JSON matching `examples/worker-output.example.json`:

```bash
rlm-mcp map-complete --plan-id <plan_id> --worker-id worker-a --batch-id batch-0 ^
  --output "{\"batch_id\":\"batch-0\",\"worker_id\":\"worker-a\",\"findings\":[{\"summary\":\"three ERROR lines\"}],\"unresolved\":[]}" --json
```

## 7. Parent reduces

Collect each worker's output (from your orchestrator or by re-reading completed batches) and merge:

```bash
rlm-mcp reduce-merge --workers "[{\"batch_id\":\"batch-0\",\"findings\":[...]}]" --json
```

## MCP equivalents

| CLI | MCP tool |
|-----|----------|
| `map-plan` | `rlm_map_plan` |
| `map-claim` | `rlm_map_claim` |
| `map-complete` | `rlm_map_complete` |
| `reduce-merge` | `rlm_reduce_merge` |

## Guarantees

- Each `batch_id` can be claimed by only one worker at a time.
- `rlm_map_complete` verifies the claiming `worker_id`.
- Plans are file-locked for Windows-safe concurrent claim/complete.
- Session chunks remain read-only; workers do not rewrite the parent session.