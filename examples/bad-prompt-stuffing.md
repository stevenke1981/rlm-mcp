# Bad pattern: prompt stuffing

## What agents do wrong

```text
User: Analyze this 500k-line log for ERROR lines.

Agent: [pastes entire log into chat context]
```

Problems:

- Blows the model context window.
- Re-reads the same bytes on every turn.
- Hides which regions were actually inspected.
- Cannot parallelize map workers.

## RLM pattern (this repo)

1. **Load externally** — `rlm_scan` stores the log under `session_id` in `RLM_CACHE_DIR`.
2. **Filter** — `rlm_peek --query ERROR` returns only matching lines + `chunk_id`.
3. **Map** — workers call `rlm_chunk` on assigned `chunk_id` batches only.
4. **Reduce** — `rlm_reduce_merge` combines small worker JSON; cite `chunk_id` evidence.

The model plans the loop; **long text stays in the environment**, not the chat transcript.

## Smell test

| Symptom | Fix |
|---------|-----|
| Chat contains full file contents | Use `rlm_scan` + `rlm_peek` |
| Re-uploading files each turn | Reuse `session_id` via `rlm_session_list` |
| No provenance in final answer | Require `chunk_ids` / `paths` in worker JSON |
| Runaway cost on recursion | Configure `rlm_budget_configure` + `rlm_task_cancel` |