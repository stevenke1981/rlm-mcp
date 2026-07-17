# Agent prompt templates

Copy-paste system/task prompts that steer an agent through the RLM loop
(`load → filter → map → reduce → recurse`) without stuffing context into chat.

All templates assume the **rlm-mcp** MCP server is connected. Replace
`<session_id>` and `<chunk_id>` with values returned by the tools.

---

## 1. Generic long-context analyst

```text
You have the rlm-mcp tools. Context is EXTERNAL — never paste file contents
into your reasoning. Follow this loop:

1. rlm_scan(path=<target>) once. Save session_id.
2. rlm_peek(session_id, query=<keyword>) to locate relevant chunks.
   Prefer bm25=true for "most relevant" ranking on prose/logs.
3. rlm_chunk(session_id, chunk_id=<id>, limit<=5) only for chunks that peek
   proved relevant.
4. Reduce findings to structured JSON with chunk_ids as provenance.
5. Only re-run peek→chunk for gaps you can name.
6. rlm_trajectory_final(session_id, answer, evidence_count) when done.

Never read more than 5 chunks per call. Stop and report if budget is exceeded.
```

---

## 2. Huge log diagnosis

```text
Goal: find the root cause of <symptom> in the loaded logs.

1. rlm_scan(path=<log dir>). Save session_id.
2. rlm_peek(session_id, query="ERROR", bm25=false) — exact keyword sweep.
3. rlm_peek(session_id, query="<symptom terms>", bm25=true) — ranked lines.
4. For the top chunk_ids, rlm_chunk(..., include_content=true, limit=3).
5. Build a timeline: earliest error → cascade → final failure.
6. Reduce to JSON: {root_cause, evidence_chunk_ids, timeline, confidence}.
```

See `fixtures/log-diagnosis/` for a runnable mini corpus.

---

## 3. Multi-document research (parallel map)

```text
Goal: answer <question> across many documents.

1. rlm_scan(path=<docs dir>). Save session_id.
2. rlm_peek(session_id, query=<question terms>, bm25=true, limit=20).
3. rlm_map_plan(session_id, chunk_ids=[...from peek...], batch_size=3).
4. For each batch: rlm_map_claim(plan_id, worker_id=<unique>), read chunks,
   rlm_map_complete(plan_id, worker_id, batch_id, output=<worker JSON>).
5. rlm_reduce_merge(worker_outputs=[...]) → consolidated findings.
6. If a sub-question is unresolved, recurse: rlm_task_create over the
   relevant chunk_ids.
```

See `parallel-workers.md` and `multi-document-research.md`.

---

## 4. Repository QA without graph tools

```text
Goal: explain how <feature> works in this codebase.

1. rlm_scan(path=src). Save session_id.
2. rlm_peek(session_id, query="<symbol or feature>", bm25=true).
3. rlm_peek(session_id, glob="**/*.rs", query="fn <name>", regex=true) to
   pin definitions.
4. rlm_chunk the top matches; trace call sites via more peeks.
5. Reduce to: {entry_points, key_functions, data_flow, evidence_chunk_ids}.

Note: for symbol graphs / call-path tracing, prefer a dedicated code-graph
MCP server. rlm-mcp is text-first.
```

---

## 5. Long transcript / pairwise aggregation

```text
Goal: aggregate <metric> mentioned across a long transcript.

1. rlm_scan(path=<transcript>). Save session_id.
2. rlm_peek(session_id, query="<metric marker>", bm25=true, limit=50).
3. rlm_chunk the matched chunks in small batches.
4. rlm_transform(session_id, op="filter_lines", params={"query":"METRIC"})
   to isolate structured lines into an artifact.
5. Sum / compare in your reduce step; cite chunk_ids as evidence.
```

---

## Budget discipline (all templates)

```text
Before a large run:
  rlm_budget_configure(session_id, max_chunks_read=..., max_sub_calls=...,
                       mode="fail_fast")
Check anytime:
  rlm_budget_status(session_id)  # watch tail_cost.high_variance
```

The paper warns recursive runs have high tail-cost variance. For small/simple
contexts a direct answer may be cheaper than the full RLM loop — judge first.
