# Limitations — when rlm-mcp is not the best choice

This document states known limits of **rlm-mcp** relative to the [Recursive Language Models paper](https://arxiv.org/abs/2512.24601) and to simpler alternatives. It complements [`paper-mapping.md`](paper-mapping.md) (what we implement) with honest guidance on **when not to use** the full RLM loop.

---

## 1. Small or simple contexts — direct calls may win

**Paper observation:** RLM overhead can exceed benefit when the entire prompt fits comfortably in the model context window and the task is a single-pass question.

**In rlm-mcp:**

- The S-NIAH `mini` fixture often shows `direct_full_context` as **correct** with the smallest `bytes_in` when the haystack is tiny (~40 filler lines each side of the needle).
- `summary_compaction` intentionally **fails** on buried needles — it only reads head/tail lines and misses middle content (`tests/benchmark_sniah.rs`).
- For a 2-page PDF or a short log, stuffing the text into one model call (or a single `rlm_scan` + one `rlm_chunk`) is often faster and cheaper than filter → map → reduce → optional recursion.

**Guidance:**

| Situation | Prefer |
|-----------|--------|
| Fits in context, one-shot QA | Direct model call or single chunk read |
| Needle buried in long haystack | `rlm_peek` → targeted `rlm_chunk` |
| Multi-file corpus, unknown location | Full RLM loop |

See also: [`examples/bad-prompt-stuffing.md`](../examples/bad-prompt-stuffing.md) — the anti-pattern is stuffing *large* context, not using RLM on *small* context when unnecessary.

---

## 2. High tail cost on recursive trajectories

**Paper observation:** Median RLM cost can be comparable to baselines, but **tail latency and token use vary widely** when recursion depth or fan-out grows.

**In rlm-mcp:**

- `rlm_budget_status` reports `tail_cost.high_variance` when trajectory sub-call byte costs show p90 > 2× p50 (`src/rlm/budget.rs`).
- `rlm_with_subcalls` baseline records `sub_call_count > 0` and typically more trajectory events than `rlm_no_subcalls`.
- Budget limits (`max_sub_calls`, `max_wall_secs`, `max_total_tokens_est`) stop runaway trees; `rlm_task_cancel` aborts in-flight work.

**Guidance:**

- Configure budgets **before** deep recursion: `rlm_budget_configure`.
- Inspect runs after failure: `rlm_trajectory_get`, JSONL export.
- Prefer **agent-managed** shallow loops (one filter/map/reduce pass) unless information density justifies `rlm_task_create` trees.

---

## 3. Provider-backed recursion is optional and limited

**Paper observation:** Behavior depends on model choice; recursive sub-calls invoke sub-models over snippets.

**In rlm-mcp today:**

| Provider | Status | Use |
|----------|--------|-----|
| `mock` | Shipped | Deterministic CI / offline tests |
| `dry-run` | Shipped | Plan sub-calls without token spend |
| OpenAI-compatible | Shipped (opt-in) | `RLM_ALLOW_NETWORK=1` + `RLM_OPENAI_API_KEY` |
| Local command | Shipped | `RLM_PROVIDER_COMMAND` (+ optional `RLM_PROVIDER_ARGS`) |

- Default integration path: **host agent** plans filter/map/reduce; long text never leaves the session unless you explicitly chunk or call a provider.
- `rlm_task_*` with `provider: mock` proves the recursion *protocol*, not production model quality.
- No dollar-cost accounting until provider hooks land (TODO P1/P2).

**Guidance:** Treat live model recursion as opt-in. Local tests and CI require **no API keys**.

---

## 4. Semantic tasks need better planning — tools alone are not enough

**Paper observation:** Information-dense tasks benefit from decomposition; vague prompts waste recursion budget.

**In rlm-mcp:**

- `rlm_peek` supports **substring / regex / glob / BM25** token ranking; it is not semantic embedding search.
- `rlm_reduce_merge` merges structured JSON; it does not infer meaning from unstructured worker prose.
- Repository understanding without symbol graphs relies on text search — pair with **`cbm-mcp`** for call-path / symbol tasks (separate MCP, no code coupling).

**Failure modes:**

| Symptom | Cause | Mitigation |
|---------|-------|------------|
| Peek returns no matches | Query terms absent in corpus | Broaden query, try path/glob, slice suspected regions |
| Reduce says `needs_recursion` forever | Workers return empty findings | Tighten map prompts; verify `chunk_id` evidence |
| Wrong answer despite correct retrieval | Model misread chunk | Second pass on cited `chunk_ids` only |

**Guidance:** Use `rlm_workflow` phase hints and [`rlm-loop.md`](rlm-loop.md). Require worker JSON with `chunk_ids`, `paths`, and `confidence` per [`examples/worker-output.example.json`](../examples/worker-output.example.json).

---

## 5. Intentional differences from the paper REPL

These are **design choices**, not bugs:

1. **No default Python REPL** — MCP tools replace `prompt` as a Python variable; executable sandboxes are P2.
2. **No graph index** — code structure questions should use `cbm-mcp`, not `rlm_peek` alone.
3. **Benchmark scope** — CI runs S-NIAH, OOLONG, and CodeQA `mini` suites; BrowseComp-Plus and OOLONG-Pairs remain `planned` in `rlm_benchmark_list`.
4. **Retrieval baseline ≠ BM25** — `retrieval_peek` uses `rlm_peek` substring filter; CodeAct + BM25 from the paper is not replicated yet.

---

## 6. Operational limits

| Limit | Config / behavior |
|-------|-------------------|
| Max file / session size | `RLM_MAX_FILE_BYTES`, `RLM_MAX_TOTAL_BYTES` |
| Max chunks / sessions | `RLM_MAX_CHUNKS`, `RLM_MAX_SESSIONS` |
| Session expiry | `RLM_SESSION_TTL_SECS`; `rlm_session_cleanup` |
| Binary / oversized files | Skipped with `skip_reasons` in scan metadata |
| Concurrent writers | Per-session lock; readers use `get_or_hydrate` |
| Cross-process reads | Sessions on disk under `RLM_CACHE_DIR` |

---

## 7. Quick decision checklist

Use the **full RLM loop** when:

- Context is **larger than safe model window** or should not enter chat history.
- You need **provenance** (`chunk_id`, trajectory, budget report).
- Work can be **parallelized** across chunk batches.
- Information is **sparse** in a large haystack (needle-in-haystack, log errors, multi-doc research).

Skip or shorten the loop when:

- Entire input fits one model call with room to spare.
- Task is purely **semantic** and substring peek cannot locate candidates (wait for BM25/embeddings or use external retrieval).
- You need **symbol-level** code navigation (use `cbm-mcp`).
- You require **live recursive sub-models** today without building agent-side provider calls.

---

## Related docs

- [`paper-mapping.md`](paper-mapping.md) — concept ↔ implementation status
- [`benchmarks.md`](benchmarks.md) — reproducible mini-suite and baseline interpretation
- [`rlm-loop.md`](rlm-loop.md) — end-to-end walkthrough