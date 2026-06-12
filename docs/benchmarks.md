# Benchmarks — S-NIAH mini-suite

Offline evaluation harness for qualitative claims from the [RLM paper](https://arxiv.org/abs/2512.24601). Runs in CI without API keys.

Implementation: `src/benchmark/` · Tests: `tests/benchmark_sniah.rs`

---

## Available suites

```powershell
rlm-mcp benchmark-list --json
# MCP: rlm_benchmark_list
```

| Suite | Status | CI default | Fixture sizes |
|-------|--------|------------|---------------|
| `sniah` | **Runnable** | `mini` | `mini`, `small`, `large`, `nightly` |
| `oolong` | **Runnable** | `mini` | `mini`, `small` |
| `codeqa` | **Runnable** | `mini` | `mini`, `small` |
| `browsecomp_plus` | Planned | — | — |
| `oolong_pairs` | Planned | — | — |

---

## CodeQA-style (repository symbol lookup)

**Task:** A synthetic mini-repo is scanned from disk. Baselines must find the `pub fn <symbol>` name in `src/pipeline.rs`.

**Fixture generation** (`src/benchmark/codeqa.rs`):

| Size | Files | Typical use |
|------|-------|-------------|
| `mini` | 5 | CI, fast local check |
| `small` | 12 | Local regression |

```powershell
rlm-mcp benchmark-run --suite codeqa --fixture-size mini --json
```

---

## OOLONG-like (metric aggregation)

**Task:** Each synthetic document contains one `METRIC=<n>` line. Baselines must return the **sum** of all metrics across documents (not a single needle value).

**Fixture generation** (`src/benchmark/oolong.rs`):

| Size | Documents | Typical use |
|------|-----------|-------------|
| `mini` | 6 | CI, fast local check |
| `small` | 15 | Local regression |

Compaction reads only head/tail lines and typically **under-counts** scattered metrics.

```powershell
rlm-mcp benchmark-run --suite oolong --fixture-size mini --json
```

---

## S-NIAH (Synthetic Needle In A Haystack)

**Task:** A unique `NEEDLE_KEY=MAGIC-<uuid>` line is buried in synthetic filler text. Each baseline must recover the magic value.

**Fixture generation** (`src/benchmark/sniah.rs`):

| Size | Filler lines (each side of needle) | Typical use |
|------|-----------------------------------|-------------|
| `mini` | 40 | CI, fast local check |
| `small` | 200 | Local regression, slightly harder |
| `large` | 2,000 | Local stress / tail-cost inspection |
| `nightly` | 8,000 | Scheduled nightly workflow only |

Needle is placed at the **middle line** — compaction baselines that only read head/tail miss it by design.

---

## Baselines

All five map to `BaselineKind` in `src/benchmark/types.rs`:

| Baseline ID | What it simulates | Expected on `mini` |
|-------------|-------------------|---------------------|
| `direct_full_context` | Paste full haystack into model context | Correct; highest `bytes_in` |
| `summary_compaction` | Read first/last ~10% of lines only | **Incorrect** (misses buried needle) |
| `retrieval_peek` | `rlm_scan` + `rlm_peek` (substring, no full chunk body) | Correct; lower `bytes_in` than direct |
| `rlm_no_subcalls` | Filter → `rlm_chunk` → `rlm_reduce_merge` | Correct |
| `rlm_with_subcalls` | Above + `rlm_task_create` with `mock` provider | Correct; `sub_call_count > 0` |

**Note:** `retrieval_peek` approximates paper retrieval/BM25 via substring peek — not a true BM25 index (see [`limitations.md`](limitations.md)).

---

## Metrics

Recorded per baseline in `metrics`:

| Field | Meaning |
|-------|---------|
| `correct` | Extracted answer equals `needle_value` |
| `bytes_in` | **Model-visible** context bytes (evidence string), not full session storage load |
| `bytes_out` | Model-visible output bytes |
| `tokens_est` | Rough `(bytes)/4` estimate |
| `runtime_ms` | Wall time for baseline run |
| `trajectory_events` | Events recorded when session-backed |
| `sub_call_count` | Recursive task invocations |
| `chunks_read` | Chunks touched via engine |

**Important:** Cost comparisons use model-visible bytes (Lesson #5 in `lessons.md`). Do not compare against `total_bytes` from `rlm_scan` alone.

Report `summary` includes:

```json
{
  "accuracy": { "correct": 4, "total": 5 },
  "qualitative_claims": {
    "retrieval_lower_cost_than_direct": true,
    "summary_compaction_misses_buried_needle": true,
    "rlm_subcalls_higher_variance": true
  },
  "paper_note": "Median costs comparable; inspect tail via trajectory sub_call and budget events"
}
```

---

## How to run

### CLI

```powershell
# List suites
rlm-mcp benchmark-list --json

# CI-sized run (default)
rlm-mcp benchmark-run --suite sniah --fixture-size mini --json

# Larger local run
rlm-mcp benchmark-run --suite sniah --fixture-size small --json

# Stress run (optional; slower)
rlm-mcp benchmark-run --suite sniah --fixture-size large --json
```

### MCP

```json
{ "name": "rlm_benchmark_run", "arguments": { "suite": "sniah", "fixture_size": "mini" } }
```

### Cargo test (CI)

```powershell
cargo test --test benchmark_sniah
```

### Optional local / nightly fixtures

```powershell
# Local regression (ignored in CI)
cargo test sniah_small_suite --test benchmark_sniah -- --ignored

# Large stress fixtures
cargo test sniah_large_suite --test benchmark_sniah -- --ignored

# Nightly-scale fixtures (also run on schedule via .github/workflows/nightly.yml)
cargo test sniah_nightly_suite --test benchmark_sniah -- --ignored
```

Assertions in `sniah_mini_suite_runs_all_baselines`:

- 5 baselines run
- `direct_full_context` correct
- `summary_compaction` incorrect
- `retrieval_peek`, `rlm_no_subcalls`, `rlm_with_subcalls` correct with `session_id`
- `retrieval_peek` `bytes_in` < `direct_full_context` `bytes_in`
- Summary accuracy: 4/5 correct

---

## Interpreting results

### Claims the mini-suite supports

1. **External context + retrieval beats stuffing** — peek baseline reads far fewer bytes than direct while staying correct.
2. **Compaction loses buried facts** — head/tail summary misses middle needle (paper motivation for programmatic examination).
3. **RLM loop works offline** — filter/map/reduce path finds needle without provider credentials.
4. **Sub-calls add trajectory cost** — `rlm_with_subcalls` records sub-call events; use budget/trajectory tools for tail analysis.

### Claims the mini-suite does *not* fully prove

- BrowseComp-Plus, OOLONG, OOLONG-Pairs, CodeQA task families (adapters planned).
- True BM25 / CodeAct baselines from the paper.
- Live model quality across providers (only `mock` in harness).
- Large-scale tail latency distributions (use `large` / `nightly` + budget/trajectory tools).

---

## Tail cost and budgets

After a session-backed baseline, inspect:

```powershell
rlm-mcp budget-status --session-id <session_id> --json
rlm-mcp trajectory-get --session-id <session_id> --json
```

Look for `tail_cost.high_variance` and `paper_note` in budget status when sub-call byte costs spread widely — mirrors the paper's caution on recursive runs.

---

## Adding future suites (maintainers)

1. Add module under `src/benchmark/`.
2. Register in `list_suites()` and `run_suite()` in `src/benchmark/mod.rs`.
3. Add integration test under `tests/`.
4. Document fixture sizes and baselines here.
5. Keep suites **offline** for CI unless explicitly marked optional/nightly.

---

## Related docs

- [`limitations.md`](limitations.md) — when benchmarks overstate production readiness
- [`paper-mapping.md`](paper-mapping.md) §8 — paper benchmark ↔ adapter table
- [`rlm-loop.md`](rlm-loop.md) — manual loop matching `rlm_no_subcalls` / `rlm_with_subcalls` paths