# TODO - rlm-mcp full RLM paper implementation

Official Rust MCP SDK migration plan: [`RMCP_MIGRATION_TODO.md`](RMCP_MIGRATION_TODO.md).

Goal: make `D:\rlm-mcp` an independent Rust MCP project that faithfully presents the Recursive Language Models paper as a usable agent runtime.

Reference sources:

- Paper: Recursive Language Models, arXiv 2512.24601
- Official code reference: `https://github.com/alexzhang13/rlm`
- Public target repo: `https://github.com/stevenke1981/rlm-mcp.git`
- This repo must stay independent from `D:\cbm-mcp`.
- Graph/codebase-memory tools are out of scope for this repo.

Definition of done:

- The server remains a standalone RLM MCP server.
- It exposes the paper's core idea: long context is external environment state, not prompt stuffing.
- Agents can load, inspect, decompose, recursively dispatch work, and reduce results with controlled budgets.
- The implementation includes examples, tests, and evaluation harnesses that demonstrate the paper's claims and limitations.

## Paper facts to preserve

- RLM treats long prompts as part of an external environment.
- The model programmatically examines and decomposes the prompt.
- The model can recursively call itself or sub-models over snippets.
- The interface should feel like an LM: input string in, output string out, but internally it uses external context and recursive computation.
- The paper's REPL instantiation loads the prompt as a variable inside a Python environment.
- Evaluation covers tasks including S-NIAH, BrowseComp-Plus, OOLONG, OOLONG-Pairs, and CodeQA.
- Important baselines include direct long-context calls, summary/compaction agents, CodeAct + BM25, RLM with REPL, and RLM with REPL but no sub-calls.
- Important observations to model:
  - external context enables scaling past model context windows
  - recursive sub-calls help on information-dense tasks
  - costs are comparable on median but high variance at the tail
  - RLM can be worse than direct calls for small/simple contexts
  - model choice affects behavior

## P0 - Product boundary and naming

- [x] Keep this repo independent from `cbm-mcp`.
- [x] Keep MCP server name `rlm-mcp` (renamed from `codebase-memory-rlm-mcp` in all templates and docs).
- [x] Use only RLM tools here:
  - `rlm_workflow`
  - `rlm_scan`
  - `rlm_peek`
  - `rlm_chunk`
  - `rlm_session_list`
  - `rlm_session_delete`
  - `rlm_env_info`
  - `rlm_slice`
  - `rlm_transform`
  - `rlm_artifact_read`
  - `rlm_artifact_write`
  - `rlm_map_plan`
  - `rlm_map_claim`
  - `rlm_map_complete`
  - `rlm_reduce_schema`
  - `rlm_reduce_merge`
- [x] Do not add graph indexing tools to this repo.
- [x] Add README section that states when to use `rlm-mcp` alone vs together with `cbm-mcp`.
- [x] Add release/install instructions for Windows, Linux, and macOS.

Acceptance criteria:

- A user can install only `rlm-mcp` and perform long-context analysis without CBM.
- Docs never imply hidden coupling to `cbm-mcp`.

## P0 - External context environment

- [x] Treat loaded context as external state with a stable `session_id`.
- [x] Persist sessions under `RLM_CACHE_DIR`.
- [x] Support loading:
  - single files
  - directories
  - text blobs from stdin or MCP argument
  - optional named prompt/session variables
- [x] Record metadata:
  - root path
  - file count
  - chunk count
  - total bytes
  - skipped files
  - skip reasons
  - created time
  - TTL/expiry
- [x] Implement resource limits:
  - max file size
  - max total bytes
  - max chunks
  - max session count
  - max session age
- [x] Add atomic persistence and corrupt-session recovery.
- [x] Add clear errors for binary/oversized/unsupported files.

Acceptance criteria:

- Context survives across separate MCP/CLI invocations.
- Large files do not enter the model context unless explicitly chunked.

## P0 - Paper-style REPL interaction model

- [x] Add an explicit environment abstraction equivalent to the paper's REPL:
  - inspect context length
  - list files/variables
  - slice ranges
  - search substrings
  - transform selected snippets
  - produce derived artifacts
- [x] Decide execution model:
  - safe built-in transform ops (default; see `docs/repl-execution-model.md`)
  - embedded scripting sandbox (deferred P2)
  - external Python REPL mode behind an explicit flag (deferred P2)
- [x] Add MCP tools for environment interaction if needed:
  - `rlm_env_info`
  - `rlm_slice`
  - `rlm_search` (covered by enhanced `rlm_peek`)
  - `rlm_transform`
  - `rlm_artifact_read`
  - `rlm_artifact_write`
- [x] Keep default mode safe and deterministic.
- [x] Add output byte limits for safe REPL transforms/artifacts (`RLM_MAX_TRANSFORM_BYTES`, `RLM_MAX_ARTIFACT_BYTES`).
- [x] Add command/time/memory limits for executable REPL sandboxes (P2 backends; policy in `docs/repl-execution-model.md`).

Acceptance criteria:

- The agent can interact with context as an external object, not just static chunks.
- Unsafe code execution is opt-in, sandboxed, and documented.

## P0 - Filter phase

- [x] Extend `rlm_peek` beyond simple substring search:
  - path filter
  - glob filter
  - regex option
  - case sensitivity
  - line range
  - context radius
  - top-k matches
- [x] Add preview summaries:
  - file-level summary
  - chunk-level metadata
  - match counts
- [x] Add BM25 or token search option for document corpora.
- [x] Add result IDs that can be fed into map tools.

Acceptance criteria:

- Agents can narrow a 10M+ token session without reading all chunks.
- `rlm_peek` results are small, structured, and stable.

## P0 - Map phase

- [x] Extend `rlm_chunk` for map workloads:
  - batch IDs
  - file pattern
  - chunk offset/limit
  - byte/line ranges
  - include/exclude metadata
  - stable chunk IDs
- [x] Add `rlm_map_plan` to create parallel work units from peek results.
- [x] Add `rlm_map_claim` / `rlm_map_complete` if multi-agent coordination is needed.
- [x] Define expected worker output JSON schema.
- [x] Add examples for parallel agent workers.

Acceptance criteria:

- A parent agent can spawn workers over chunk batches without duplicate work.
- Worker outputs are reducible without rereading the whole session.

## P0 - Reduce phase

- [x] Add `rlm_reduce_schema` guidance for combining map outputs.
- [x] Add `rlm_reduce_merge` helper for structured JSON findings.
- [x] Support iterative filter -> map -> reduce loops when gaps remain.
- [x] Track provenance:
  - which chunks support each finding
  - which worker produced it
  - confidence
  - unresolved questions
- [x] Add final-answer checklist:
  - evidence present
  - no unvisited required regions
  - cost/budget summary

Acceptance criteria:

- Reduce outputs can cite source chunks and explain coverage.
- The agent can identify when another recursion pass is needed.

## P1 - Recursive sub-call runtime

- [x] Implement explicit recursive call abstraction:
  - root task
  - sub-task prompt
  - selected context snippets
  - model/provider configuration
  - budget
  - structured result
- [x] Add MCP tools or CLI helpers:
  - `rlm_task_create`
  - `rlm_task_list`
  - `rlm_task_result`
  - `rlm_task_reduce`
- [x] Support external agent-managed recursion first.
- [x] Optionally support provider-backed subcalls:
  - dry-run provider for tests
  - mock provider for tests
- [x] Optionally support provider-backed subcalls:
  - OpenAI-compatible API
  - local command provider
- [x] Add recursion controls:
  - max depth
  - max fanout
  - max subcalls
  - max input bytes
- [x] Add recursion controls:
  - max wall time (enforced)
  - cancellation
- [x] Add cycle/duplicate sub-task detection.

Acceptance criteria:

- The system can represent recursive decomposition explicitly.
- Tests can run without paid model calls via a deterministic mock provider.

## P1 - RLM trajectory logging

- [x] Persist trajectory events:
  - scan/load
  - peek/filter
  - chunk/map
  - sub-call
  - reduce
  - final answer
  - budget event
  - error/cancel
- [x] Add `rlm_trajectory_get`.
- [x] Add JSONL export for analysis.
- [x] Add replay mode for deterministic debugging.
- [x] Add redaction controls for sensitive text.

Acceptance criteria:

- A complete RLM run can be inspected after the fact.
- Cost and quality failures can be diagnosed from trajectory logs.

## P1 - Cost, budget, and runtime accounting

- [x] Track:
  - input bytes/tokens estimated
  - output bytes/tokens estimated
  - chunks read
  - sub-call count
  - recursion depth
  - elapsed time
- [x] Track:
  - provider cost if configured (`provider_cost_usd_est` in `rlm_budget_status`)
- [x] Add budget configuration:
  - per session
  - per task
  - per sub-call
  - per recursion tree
- [x] Add fail-fast and soft-warning modes.
- [x] Add high-variance tail cost reporting, matching the paper's caution.

Acceptance criteria:

- RLM runs report cost-like metrics even without a provider.
- A runaway recursive plan is bounded and cancellable.

## P1 - Long-context benchmarks from the paper

- [x] Add benchmark harness with adapters for:
  - S-NIAH
  - BrowseComp-Plus-like local corpus task
  - OOLONG-like aggregation task
  - OOLONG-Pairs-like pairwise aggregation task
  - CodeQA-style repository understanding task
- [x] Add small synthetic fixtures that can run in CI.
- [x] Add large optional fixtures for local/nightly runs (`large`, `nightly`; `.github/workflows/nightly.yml`).
- [x] Implement baselines:
  - direct full-context call where possible
  - summary/compaction agent
  - retrieval/BM25 agent
  - RLM without sub-calls
  - RLM with sub-calls
- [x] Record accuracy, cost estimate, runtime, and trajectory length.

Acceptance criteria:

- The repo can demonstrate the paper's qualitative claims with reproducible mini-benchmarks.
- Large benchmark runs are optional and documented.

## P1 - Task patterns and examples

- [x] Add examples for:
  - huge log diagnosis
  - multi-document research
  - repository QA without graph tools
  - long transcript analysis
  - pairwise aggregation
  - line-by-line semantic transformation
- [x] Add walkthroughs showing:
  - load
  - filter
  - map
  - reduce
  - recursive second pass
- [x] Add expected worker JSON examples.
- [x] Add bad examples showing prompt stuffing and why not to do it.

Acceptance criteria:

- A new agent can copy an example workflow and run it end-to-end.

## P1 - MCP and CLI contract

- [x] Snapshot `tools/list`.
- [x] Add CLI equivalents for every MCP tool.
- [x] Ensure `--json --quiet` produces parseable stdout.
- [x] Add process-level tests for CLI tools.
- [x] Add MCP inspector smoke:
  - `initialize`
  - `tools/list`
  - `tools/call rlm_scan`
  - `tools/call rlm_peek`
  - `tools/call rlm_chunk`
- [x] Add schema docs for every tool.

Acceptance criteria:

- Other agents can integrate without reading Rust source.

## P1 - Session storage and concurrency

- [x] Make session files atomic and Windows-safe.
- [x] Add per-session lock or optimistic concurrency.
- [x] Support multiple simultaneous agents reading the same session.
- [x] Support session deletion while reads are active safely.
- [x] Add cleanup command for expired sessions.
- [x] Add session import/export.

Acceptance criteria:

- Parallel map workers can safely read session chunks.
- Session cleanup cannot corrupt active reads.

## P2 - Optional REPL / sandbox backends

- [x] Design backend trait:
  - safe Rust environment backend (`src/rlm/repl/safe.rs`)
  - Python REPL backend (reserved stub)
  - command sandbox backend (`src/rlm/repl/command.rs`)
- [x] Add capability flags per backend.
- [x] Add allowlist-based filesystem access.
- [x] Add timeout and memory limits.
- [x] Add audit logs for executed code (`repl_exec` trajectory events).
- [x] Default to non-executable safe backend.

Acceptance criteria:

- Paper-style REPL is available without making default MCP unsafe.

## P2 - Provider abstraction

- [x] Add provider trait for sub-model calls.
- [x] Add mock provider for tests.
- [x] Add local command provider.
- [x] Add OpenAI-compatible provider behind env config.
- [x] Add retry/backoff.
- [x] Add token/cost estimation hooks.
- [x] Keep provider secrets out of session artifacts.

Acceptance criteria:

- Recursive subcalls can be tested offline and enabled online explicitly.

## P2 - Safety and privacy

- [x] Add secret redaction for trajectory exports (default markers + `redact_patterns`).
- [x] Add binary-file detection (`src/rlm/safety.rs` heuristic + scan skips).
- [x] Add max output size for transform and artifact outputs.
- [x] Add max output size for chunk reads (`RLM_MAX_CHUNK_BYTES`).
- [x] Add explicit opt-in for network/provider calls.
- [x] Add safe temp directory handling (`src/project.rs`: dedicated layout, traversal/temp guards).
- [x] Add Windows path traversal tests (`tests/path_safety.rs`).

Acceptance criteria:

- Loading large local data does not leak content to providers unless explicitly requested.

## P2 - Packaging and agent handoff

- [x] Keep install scripts idempotent.
- [x] Add manual MCP templates:
  - OpenCode
  - Codex
  - Claude-style `mcpServers`
  - generic
- [x] Add stable installed binary path.
- [x] Add release artifact smoke:
  - checksum
  - extracted binary
  - MCP initialize/tools/list
  - scan/peek/chunk
- [x] Add GitHub release workflow.
- [x] Add README install troubleshooting.

Acceptance criteria:

- A fresh agent can install and use RLM MCP without `cbm-mcp`.

## P3 - Documentation matching the paper

- [x] Add `docs/paper-mapping.md`:
  - paper concept
  - Rust/MCP implementation
  - status
  - tests/examples
- [x] Add `docs/rlm-loop.md`:
  - external context
  - filter
  - map
  - reduce
  - recursion
- [x] Add `docs/benchmarks.md`.
- [x] Add `docs/limitations.md`:
  - small/simple contexts may be worse than direct calls
  - recursive trajectories can have high tail cost
  - provider-backed recursion is optional
  - semantic tasks may require better planning

Acceptance criteria:

- The repo clearly explains how it embodies the RLM paper and where it intentionally differs.

## Verification checklist before claiming paper-complete

- [x] `cargo fmt --check`
- [x] `cargo test --all-targets`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo build --release`
- [x] MCP initialize/tools/list smoke
- [x] CLI JSON contract tests
- [x] scan/peek/chunk end-to-end fixture
- [x] multi-worker map fixture
- [x] recursive sub-task fixture with mock provider
- [x] trajectory export/replay fixture
- [x] benchmark mini-suite
- [x] install/release artifact smoke

## Non-goals

- Do not implement code graph indexing here; use `D:\cbm-mcp`.
- Do not depend on `D:\cbm-mcp` or the legacy combined `D:\cbm\cbrlm`.
- Do not require network/model-provider credentials for local tests.
