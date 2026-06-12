# RLM loop walkthrough

This document mirrors the [Recursive Language Models](https://arxiv.org/abs/2512.24601) pattern implemented in **rlm-mcp**: long context is **external state**, not prompt stuffing.

Runnable fixture: [`examples/fixtures/log-diagnosis/`](../examples/fixtures/log-diagnosis/).

## 1. Load

```powershell
rlm-mcp scan --path examples/fixtures/log-diagnosis --json
```

Save `session_id` from the response. Inspect metadata:

```powershell
rlm-mcp env-info --session-id <session_id> --json
```

**Rule:** Do not paste `app.log` into the model prompt. The session holds bytes until deleted.

## 2. Filter

Narrow to ERROR lines without reading every chunk:

```powershell
rlm-mcp peek --session-id <session_id> --query ERROR --limit 20 --json
```

Note `matches[].chunk_id` and `preview` lines. Optional line-level read:

```powershell
rlm-mcp slice --session-id <session_id> --chunk-id <chunk_id> --start 4 --end 7 --json
```

## 3. Map

Plan parallel batches (single-worker demo uses one chunk):

```powershell
rlm-mcp map-plan --session-id <session_id> --chunk-id <chunk_id> --batch-size 1 --json
```

Worker reads assigned chunks:

```powershell
rlm-mcp chunk --session-id <session_id> --chunk-id <chunk_id> --json
```

Workers return JSON matching [`examples/worker-output.example.json`](../examples/worker-output.example.json).

## 4. Reduce

```powershell
rlm-mcp reduce-schema --json
rlm-mcp reduce-merge --workers '[{"batch_id":"batch-0","worker_id":"w0","findings":[{"summary":"disk full cascade","chunk_ids":["<chunk_id>"],"paths":["app.log"],"confidence":0.9}],"unresolved":[]}]' --json
```

Check `needs_recursion`. If `true`, return to **filter** with a refined query or unvisited paths.

## 5. Recursive second pass (optional)

When gaps remain, dispatch a sub-task over the same evidence (offline mock provider):

```powershell
rlm-mcp task-create --session-id <session_id> --prompt "explain ERROR cascade root cause" --chunk-id <chunk_id> --provider mock --json
rlm-mcp task-reduce --root-id <root_id> --json
```

Record the run:

```powershell
rlm-mcp trajectory-final --session-id <session_id> --answer "disk full caused write+flush errors" --evidence-count 3 --json
rlm-mcp budget-status --session-id <session_id> --json
```

## Other task patterns (outline)

| Pattern | Load | Filter hint | Reduce focus |
|---------|------|-------------|--------------|
| Multi-document research | `rlm_scan` on directory | `rlm_peek --glob *.md --query <topic>` | Merge findings across paths |
| Repository QA (no graph) | `rlm_scan` on repo root | `--glob *.rs --query fn ` | Cite chunk paths as pseudo-symbols |
| Long transcript | `rlm_scan --content` + `--virtual-path` | Speaker or keyword peek | Timeline aggregation |
| Pairwise aggregation | Two virtual paths in one session | Peek both, map per file | Compare A vs B in reduce |
| Line semantic transform | Single large file session | `--line-start` / `--line-end` slices | Map line batches, reduce to diff |

## Anti-pattern

See [`examples/bad-prompt-stuffing.md`](../examples/bad-prompt-stuffing.md).