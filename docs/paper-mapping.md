# Paper mapping — Recursive Language Models → rlm-mcp

Reference: [Recursive Language Models (arXiv 2512.24601)](https://arxiv.org/abs/2512.24601)  
Official reference implementation: [alexzhang13/rlm](https://github.com/alexzhang13/rlm)  
This repo: standalone Rust MCP server **`rlm-mcp`** (no dependency on `cbm-mcp` or graph indexing).

Legend: **Done** = shipped and tested · **Partial** = core path works, gaps documented · **Planned** = listed in `TODO.md` · **Differs** = intentional design choice vs paper REPL

---

## Executive summary

| Paper idea | rlm-mcp embodiment |
|------------|-------------------|
| Long context is **external environment state** | `rlm_scan` → persisted `session_id` under `RLM_CACHE_DIR` |
| **Filter → map → reduce** over snippets | `rlm_peek` / `rlm_slice` → `rlm_chunk` / `rlm_map_plan` → `rlm_reduce_merge` |
| **Recursive sub-calls** over selected context | `rlm_task_create` / `rlm_task_reduce` with `mock` / `dry-run` providers |
| **Trajectory + cost** for diagnosis | `rlm_trajectory_get`, `rlm_budget_status`, JSONL export, replay |
| **Evaluation vs baselines** | S-NIAH mini-suite (`rlm_benchmark_run`) with five baseline kinds |
| **REPL over prompt variable** | **Differs**: safe MCP tools + agent-managed loop (no default Python REPL) |

Walkthrough: [`rlm-loop.md`](rlm-loop.md) · Tool reference: [`tools.md`](tools.md)

---

## 1. External context environment

| Paper concept | Rust / MCP implementation | Status | Tests / examples |
|---------------|---------------------------|--------|------------------|
| Prompt loaded as external variable, not stuffed into root context | `RlmEngine::scan` → `ScanSession` in `src/rlm/session.rs`; persist via `src/rlm/persistence.rs` | **Done** | `tests/rlm_e2e.rs`, `tests/session_storage.rs` |
| Stable session identity across invocations | `session_id`; `get_or_hydrate` for cross-process reads | **Done** | `tests/session_storage.rs` (lock, tombstone, hydrate) |
| Load files, directories, stdin/text blobs | `rlm_scan` (`path`, `text`, `stdin`) | **Done** | `tests/cli_contract.rs`, `tests/mcp_contract.rs` |
| Named prompt / session variables | `variables` on session metadata; `rlm_env_info` | **Done** | `src/rlm/env.rs` unit coverage via e2e |
| Metadata: paths, bytes, chunks, skips, TTL | `rlm_env_info`, scan response | **Done** | `tests/rlm_e2e.rs` |
| Resource limits (size, chunks, session count, age) | `src/rlm/config.rs` + env vars (`RLM_MAX_*`) | **Done** | `src/rlm/config.rs` tests |
| Atomic persistence, corrupt recovery | `persistence.rs` write-temp-rename | **Done** | `src/rlm/persistence.rs` tests |
| Session import/export, cleanup | `rlm_session_export`, `rlm_session_import`, `rlm_session_cleanup` | **Done** | `tests/session_storage.rs` |

**Acceptance:** Context survives separate MCP/CLI calls; large files stay out of model context unless explicitly chunked.

---

## 2. REPL-style interaction model

| Paper concept | Rust / MCP implementation | Status | Tests / examples |
|---------------|---------------------------|--------|------------------|
| Inspect context length | `rlm_env_info` → `context_length_bytes`, `chunk_count` | **Done** | `docs/rlm-loop.md` §1 |
| List files / variables | `rlm_env_info` → `files`, `variables` | **Done** | e2e scan + env-info |
| Slice line ranges | `rlm_slice` (`src/rlm/env.rs`) | **Done** | walkthrough §2 |
| Search substrings | `rlm_peek` (`src/rlm/filter.rs`) | **Done** | `tests/rlm_e2e.rs` |
| Transform snippets / derived artifacts | `rlm_transform`, `rlm_artifact_write`, `rlm_artifact_read` | **Done** | `src/rlm/transform.rs`, `src/rlm/artifacts.rs`, `docs/repl-execution-model.md` |
| Python REPL with prompt as variable | `rlm_repl_execute` (python stub) | **Differs** | Agent + MCP tools replace embedded REPL; command backend opt-in |
| Safe default (no arbitrary code exec) | `safe_builtin` backend; `rlm_transform` only | **Done** | `tests/repl_sandbox.rs`, transform unit tests |
| Output byte limits | `RLM_MAX_TRANSFORM_BYTES`, `RLM_MAX_ARTIFACT_BYTES` | **Done** | `docs/repl-execution-model.md` |
| Executable REPL with time/memory limits | `rlm_repl_execute` + `src/rlm/repl/` | **Done** | `tests/repl_sandbox.rs`; `RLM_REPL_MAX_WALL_SECS` |

**Acceptance:** Agents interact with context as an external object. Unsafe execution is not enabled by default.

---

## 3. Filter phase

| Paper concept | Rust / MCP implementation | Status | Tests / examples |
|---------------|---------------------------|--------|------------------|
| Narrow large corpus before full read | `rlm_peek` with `query`, `path`, `glob`, `regex`, line range, context radius, `limit` | **Done** | `src/rlm/filter.rs`, e2e peek |
| Structured small previews | Match `preview`, `chunk_id`, counts in peek response | **Done** | benchmark uses model-visible peek bytes |
| Result IDs for map phase | `chunk_id` / match IDs in peek output | **Done** | `rlm_map_plan` accepts peek-derived IDs |
| BM25 / token retrieval | `rlm_peek` with `bm25: true` (`src/rlm/bm25.rs`) | **Done** | `src/rlm/filter.rs` tests; S-NIAH `retrieval_peek` baseline |
| File/chunk summaries | Peek metadata + `rlm_env_info` file rollups | **Partial** | No separate LLM summary tool |

**Acceptance:** 10M+ token sessions can be narrowed without reading every chunk.

---

## 4. Map phase

| Paper concept | Rust / MCP implementation | Status | Tests / examples |
|---------------|---------------------------|--------|------------------|
| Paginated chunk reads | `rlm_chunk` (offset, limit, `chunk_id`, file pattern) | **Done** | `tests/rlm_e2e.rs`, multi-worker fixture |
| Parallel work units from filter hits | `rlm_map_plan` (`src/rlm/map.rs`) | **Done** | `tests/rlm_e2e.rs` |
| Worker output JSON schema | `rlm_reduce_schema`, `examples/worker-output.example.json` | **Done** | `tests/example_walkthrough.rs` |
| Multi-agent claim/complete coordination | `rlm_map_claim` / `rlm_map_complete` (`src/rlm/map_ledger.rs`) | **Done** | `tests/session_storage.rs`, `examples/parallel-workers.md` |
| Parallel worker examples | Single-worker walkthrough | **Partial** | `docs/rlm-loop.md`; dedicated multi-agent doc **Planned** |

**Acceptance:** Parent agents spawn workers over batches; workers return reducible JSON.

---

## 5. Reduce phase

| Paper concept | Rust / MCP implementation | Status | Tests / examples |
|---------------|---------------------------|--------|------------------|
| Merge structured worker findings | `rlm_reduce_merge` (`src/rlm/reduce.rs`) | **Done** | e2e + walkthrough §4 |
| Provenance (chunks, worker, confidence) | Finding fields in merge schema | **Done** | `worker-output.example.json` |
| `needs_recursion` / gap detection | Reduce output flags second pass | **Done** | `docs/rlm-loop.md` §5 |
| Final-answer checklist (evidence, budget) | `rlm_trajectory_final`, `rlm_budget_status` | **Done** | walkthrough §5 |

**Acceptance:** Reduce cites sources; agent can decide on another recursion pass.

---

## 6. Recursive sub-call runtime

| Paper concept | Rust / MCP implementation | Status | Tests / examples |
|---------------|---------------------------|--------|------------------|
| Explicit sub-task tree (prompt + snippets + budget) | `rlm_task_create`, `rlm_task_list`, `rlm_task_result`, `rlm_task_reduce` (`src/rlm/task.rs`) | **Done** | `tests/rlm_e2e.rs` recursive fixture |
| Agent-managed recursion (no provider) | Default; `provider` omitted or external | **Done** | workflow guidance |
| Provider-backed sub-calls | `SubModelProvider` trait (`src/rlm/provider/`) | **Partial** | `mock`, `dry-run`, `command`, `openai` (opt-in via `RLM_ALLOW_NETWORK`) |
| Recursion limits (depth, fanout, subcalls, bytes) | Task tree + `src/rlm/budget.rs` | **Done** | `src/rlm/task.rs`, budget tests |
| Wall time + cancellation | `rlm_task_cancel`, budget fail-fast | **Done** | `src/rlm/budget.rs` tests |
| Cycle / duplicate detection | Task tree dedup in `task.rs` | **Done** | task unit tests |

**Acceptance:** Recursion is explicit and testable offline via `mock` provider.

---

## 7. Trajectory, budget, and tail cost

| Paper concept | Rust / MCP implementation | Status | Tests / examples |
|---------------|---------------------------|--------|------------------|
| Log scan, peek, chunk, sub-call, reduce, errors | `src/rlm/trajectory.rs` event stream | **Done** | `src/rlm/trajectory.rs` tests |
| Inspect run after the fact | `rlm_trajectory_get`, JSONL export | **Done** | e2e trajectory fixture |
| Replay + redaction | Trajectory replay mode, redact flags | **Done** | trajectory module tests |
| Bytes/tokens/chunks/subcalls/time | `rlm_budget_status`, per-event metrics | **Done** | budget tests |
| Per-session / per-task / per-tree budgets | `rlm_budget_configure` | **Done** | e2e budget enforcement |
| High-variance tail cost reporting | Budget summary includes tail warnings | **Done** | aligns with paper caution (Lesson #5) |
| Dollar cost from provider | — | **Planned** | TODO P1/P2 |

**Acceptance:** Runs report cost-like metrics without a paid provider; runaway plans are bounded.

---

## 8. Evaluation and baselines (paper § experiments)

| Paper benchmark | rlm-mcp adapter | Status | Tests / examples |
|-----------------|-----------------|--------|------------------|
| **S-NIAH** | `src/benchmark/sniah.rs`, `rlm_benchmark_run` suite `sniah` | **Done** | `tests/benchmark_sniah.rs`, CI `mini`; optional `large`/`nightly` |
| **OOLONG-like** | `src/benchmark/oolong.rs`, suite `oolong` | **Done** | `tests/benchmark_oolong.rs`, CI `mini` |
| BrowseComp-Plus-like | Listed in `rlm_benchmark_list` → `planned` | **Planned** | — |
| OOLONG-Pairs | `planned` in `list_suites()` | **Planned** | — |
| **CodeQA-style** | `src/benchmark/codeqa.rs`, suite `codeqa` | **Done** | `tests/benchmark_codeqa.rs`, CI `mini` |

| Paper baseline | `BaselineKind` in `src/benchmark/types.rs` | Status |
|----------------|--------------------------------------------|--------|
| Direct long-context call | `direct_full_context` | **Done** (S-NIAH) |
| Summary / compaction agent | `summary_compaction` | **Done** (heuristic compaction in harness) |
| Retrieval / BM25 agent | `retrieval_peek` | **Partial** (peek-based, not BM25 index) |
| RLM without sub-calls | `rlm_no_subcalls` | **Done** |
| RLM with sub-calls | `rlm_with_subcalls` | **Done** |
| CodeAct + BM25 | — | **Planned** (needs REPL + BM25) |
| RLM REPL (paper native) | — | **Differs** (tool-based loop) |

Metrics recorded: accuracy, `bytes_in` / `bytes_out` (model-visible), `tokens_est`, `runtime_ms`, `trajectory_events`, `sub_call_count`.

---

## 9. Paper observations → documented behavior

| Observation (paper) | How rlm-mcp reflects it | Where |
|---------------------|-------------------------|-------|
| External context scales past context windows | Sessions persist multi-MB corpora; peek/chunk read subsets only | README, `rlm-loop.md` |
| Recursive sub-calls help on information-dense tasks | `rlm_with_subcalls` baseline vs `rlm_no_subcalls` in S-NIAH | `tests/benchmark_sniah.rs` |
| Median cost comparable, **high tail variance** | Budget tail reporting, trajectory length metrics | `rlm_budget_status`, benchmark summary |
| RLM can be **worse on small/simple** contexts | Direct baseline often wins on `mini` S-NIAH | benchmark report `summary` |
| Model choice affects behavior | Provider abstraction; today `mock` only | **Partial** until OpenAI/local provider (P2) |
| Prompt stuffing is the anti-pattern | `examples/bad-prompt-stuffing.md` | examples |

Full limitations write-up: [`limitations.md`](limitations.md). Benchmark guide: [`benchmarks.md`](benchmarks.md).

---

## 10. MCP / CLI surface (33 tools)

All paper-loop phases are exposed as MCP tools with CLI equivalents (`src/cli.rs`). Contract tests:

| Area | Test file |
|------|-----------|
| MCP initialize / tools/list / scan / peek / chunk | `tests/mcp_contract.rs` |
| CLI `--json` stdout | `tests/cli_contract.rs` |
| tools/list snapshot | `src/mcp/server.rs` (`write_tools_snapshot`) |
| Schema docs | `docs/tools.md`, `rlm_tools_reference` |

Snapshot: `packaging/mcp/tools-list.snapshot.json` (33 tools).

---

## 11. Intentional differences from the paper REPL

1. **No default Python REPL** — agents call typed MCP tools instead of executing arbitrary notebook code; optional sandboxes are P2.
2. **Recursion is often agent-orchestrated** — the host LLM plans filter/map/reduce; `rlm_task_*` adds explicit sub-call trees when needed.
3. **No graph index** — repository understanding examples use `rlm_peek` / `rlm_chunk` only; use **`cbm-mcp`** separately for symbols/call paths.
4. **Offline-first tests** — CI does not require API keys; `mock` / `dry-run` providers stand in for live models.
5. **Benchmark scope** — CI runs S-NIAH `mini`; other paper benchmarks are adapter stubs until fixtures land.

---

## 12. Remaining work (from `TODO.md`)

Priority gaps that prevent calling the repo “paper-complete”:

| Priority | Item |
|----------|------|
| P2 | BrowseComp/OOLONG/CodeQA benchmark adapters (REPL sandboxes shipped) |
| P1 | BrowseComp/OOLONG/CodeQA adapters (large S-NIAH fixtures shipped) |
| P2 | OpenAI-compatible provider; GitHub release workflow; release artifact smoke |
| P3 | — (limitations + benchmarks docs shipped) |

---

## 13. Verification commands

```powershell
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
cargo test --test release_smoke --release
cargo test --test example_walkthrough
cargo test --test benchmark_sniah
```

These match the “Verification checklist” in root `TODO.md`.