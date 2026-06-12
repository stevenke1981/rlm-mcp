# rlm-mcp examples

Copy-paste workflows for agents integrating via MCP or CLI.

| Example | Path | Description |
|---------|------|-------------|
| Log diagnosis | [`fixtures/log-diagnosis/`](fixtures/log-diagnosis/) | ERROR lines in a multi-file mini corpus |
| Worker JSON | [`worker-output.example.json`](worker-output.example.json) | Expected map-phase worker output |
| Parallel workers | [`parallel-workers.md`](parallel-workers.md) | Multi-agent claim/complete coordination |
| Anti-pattern | [`bad-prompt-stuffing.md`](bad-prompt-stuffing.md) | Why not to paste long context into chat |

Full walkthrough: [`docs/rlm-loop.md`](../docs/rlm-loop.md).

## Quick start (log diagnosis)

```powershell
cd D:\rlm-mcp
rlm-mcp scan --path examples/fixtures/log-diagnosis --json
# → session_id

rlm-mcp peek --session-id <id> --query ERROR --json
# → chunk_id from matches

rlm-mcp map-plan --session-id <id> --chunk-id <chunk_id> --batch-size 1 --json
rlm-mcp chunk --session-id <id> --chunk-id <chunk_id> --json
rlm-mcp reduce-merge --workers '[{"batch_id":"batch-0","findings":[{"summary":"disk errors","chunk_ids":["<chunk_id>"],"paths":["app.log"]}],"unresolved":[]}]' --json
```

See [`worker-output.example.json`](worker-output.example.json) for the full worker shape.