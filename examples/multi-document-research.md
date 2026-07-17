# Multi-document research with BM25 + parallel map

Answer a question that spans many documents, using `rlm_peek --bm25` to rank
the most relevant lines, then parallel map/reduce to consolidate evidence.

This mirrors the paper's "information-dense corpus" setting where recursive
sub-calls help more than a single long-context call.

## 0. When to use this

- Many files / long documents, question needs evidence from several places.
- You want *ranked* relevance, not just keyword hits → use `--bm25`.
- For tiny inputs, answer directly instead (RLM overhead is not worth it).

## 1. Load the corpus

```powershell
rlm-mcp scan --path docs --json
# → session_id, chunk_count, total_bytes
```

## 2. Rank the most relevant lines (BM25)

```powershell
rlm-mcp peek --session-id <session_id> --query "budget tail cost variance" --bm25 --limit 20 --json
```

`search_mode` will be `"bm25"`. Each match carries a `bm25_score` and a
`chunk_id`. Collect the top `chunk_id` values.

## 3. Plan parallel batches

```powershell
rlm-mcp map-plan --session-id <session_id> --chunk-id <id1> --chunk-id <id2> --chunk-id <id3> --batch-size 2 --json
# → plan_id, batches[]
```

## 4. Workers claim, read, complete

Each worker uses a unique `worker_id`:

```powershell
rlm-mcp map-claim --plan-id <plan_id> --worker-id worker-a --json
rlm-mcp chunk --session-id <session_id> --chunk-id <chunk_id> --json
rlm-mcp map-complete --plan-id <plan_id> --worker-id worker-a --batch-id batch-0 `
  --output "{\"batch_id\":\"batch-0\",\"worker_id\":\"worker-a\",\"findings\":[{\"summary\":\"tail cost is high-variance\",\"chunk_ids\":[\"<chunk_id>\"],\"paths\":[\"limitations.md\"],\"confidence\":0.8}],\"unresolved\":[]}" --json
```

## 5. Reduce

```powershell
rlm-mcp reduce-merge --workers "[{\"batch_id\":\"batch-0\",\"findings\":[{\"summary\":\"tail cost is high-variance\",\"chunk_ids\":[\"<chunk_id>\"]}],\"unresolved\":[]}]" --json
```

## 6. Recurse only for named gaps

If `unresolved` is non-empty, run a second focused pass over just those
chunks (peek → chunk), or dispatch a recursive sub-task:

```powershell
rlm-mcp task-create --session-id <session_id> --prompt "resolve: <gap>" --chunk-id <chunk_id> --provider mock --json
rlm-mcp task-reduce --root-id <root_id> --json
```

## MCP equivalents

| CLI | MCP tool |
|-----|----------|
| `scan` | `rlm_scan` |
| `peek --bm25` | `rlm_peek` (`bm25=true`) |
| `map-plan` | `rlm_map_plan` |
| `map-claim` | `rlm_map_claim` |
| `map-complete` | `rlm_map_complete` |
| `reduce-merge` | `rlm_reduce_merge` |
| `task-create` | `rlm_task_create` |
| `task-reduce` | `rlm_task_reduce` |

## Notes

- BM25 ranks by term relevance; exact substring/regex still available without
  `--bm25` for precise matches.
- `bm25` requires a `query` and is mutually exclusive with `regex`.
- Keep `limit` small on `chunk` reads; let `peek` do the narrowing.
