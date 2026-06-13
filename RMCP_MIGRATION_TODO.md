# Official rmcp MCP Server Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the standalone RLM server's custom JSON-RPC/stdio implementation with the official Rust MCP SDK while preserving all 33 RLM tools, persistent sessions, recursive task behavior, safety limits, installation paths, and release usability.

**Architecture:** Keep `ToolHandler` and the RLM environment/task/session modules as domain logic. The first migration stage uses an official `ServerHandler` adapter for negotiation, stdio, capabilities, and errors while preserving the locked 33-tool snapshot. Migrate tool families incrementally to typed Schemars routing, then add request cancellation propagation.

**Tech Stack:** Rust 2021, `rmcp 1.7.0`, Tokio, Serde, Schemars, persistent RLM sessions, optional provider and sandbox backends.

---

## Status and hard decisions

Implementation snapshot (2026-06-13): official `rmcp` `ServerHandler`, stdio,
capability negotiation, 33-tool listing/calling, domain error results, official
client contract tests, and rebuilt release-binary handshake smoke are complete.

Implementation snapshot (2026-06-13, next stage): all 33 RLM MCP tools are now
registered through the official `#[tool_router]` / `#[tool_handler]` path with
typed Schemars input structs and generated schemas. `rlm_tools_reference` and
the packaging tool snapshot now read the generated router metadata instead of
hand-written schema definitions. Router methods accept the SDK cancellation
token and bound active tool calls when MCP `notifications/cancelled` arrives;
subprocess/provider hard termination remains a follow-up for command/openai
backends that block inside domain code.

- [x] Use the current official stable SDK: `rmcp = 1.7.0`.
- [x] Pin the SDK in `Cargo.lock`; update it only through a dedicated dependency change and protocol regression run.
- [x] Use `rmcp` stdio transport as the only MCP transport implementation.
- [x] Keep binary and MCP server name `rlm-mcp`.
- [x] Keep RLM independent from `D:\cbm-mcp`; do not add graph indexing tools or graph storage dependencies.
- [x] Preserve the complete 33-tool surface recorded in `packaging/mcp/tools-list.snapshot.json`.
- [x] Keep `RLM_CACHE_DIR` sessions and artifacts compatible across the migration.
- [x] Keep network/provider calls opt-in and keep sandbox execution disabled by default.
- [x] Write logs only to stderr; stdout is reserved for MCP frames.
- [x] Do not advertise capabilities that are not implemented and tested.

## Pre-migration implementation replaced in stage 1

- `src/mcp/server.rs` previously parsed JSON-RPC and hard-coded protocol version `2024-11-05`; protocol handling now uses `rmcp`.
- `src/mcp/transport.rs` previously implemented framing and has been removed.
- `src/mcp/tools.rs` combines a reusable domain dispatcher with more than 400 lines of hand-written tool schema definitions.
- `src/mcp/schema_docs.rs` documents the public contract and must remain aligned with generated schemas.
- Existing end-to-end coverage includes `tests/mcp_contract.rs`, `tests/cli_contract.rs`, `tests/rlm_e2e.rs`, `tests/session_storage.rs`, and `tests/release_smoke.rs`.

## Target file map

- Modify: `Cargo.toml` - add official SDK, Tokio, and schema dependencies.
- Modify: `src/main.rs` - run MCP mode from an async Tokio entrypoint.
- Replace: `src/mcp/server.rs` - implement `ServerHandler`, capabilities, metadata, lifecycle, and stdio serving.
- Create: `src/mcp/router.rs` - define `#[tool_router]` methods and common result/error conversion.
- Create: `src/mcp/params/mod.rs` - export typed inputs by tool family.
- Create: `src/mcp/params/session.rs` - scan, list, cleanup, import/export, and delete inputs.
- Create: `src/mcp/params/context.rs` - peek, chunk, environment, slice, transform, and artifact inputs.
- Create: `src/mcp/params/map_reduce.rs` - map plan/claim/complete and reduce inputs.
- Create: `src/mcp/params/task.rs` - recursive task, provider, cancellation, and budget inputs.
- Create: `src/mcp/params/operations.rs` - workflow, REPL, trajectory, tools reference, and benchmark inputs.
- Modify: `src/mcp/tools.rs` - keep domain dispatch; remove manual protocol schemas after typed parity.
- Modify: `src/mcp/schema_docs.rs` - derive or verify docs against SDK-generated contracts.
- Modify: `src/mcp/mod.rs` - export the new server/router/parameter modules.
- Delete after parity: `src/mcp/transport.rs`.
- Create: `tests/rmcp_protocol.rs` - SDK negotiation, tools, calls, cancellation, and errors.
- Modify: `tests/mcp_contract.rs` - lock the generated 33-tool contract.
- Modify: `tests/release_smoke.rs` - verify the extracted release binary over stdio.
- Modify: `scripts/package-release.ps1` and `scripts/package-release.sh` only if packaging needs new runtime files; the final release should remain a single binary.
- Modify: `README.md`, `docs/tools.md`, and `packaging/mcp/README.md` - document the official SDK boundary and troubleshooting.

## P0 Task 1 - Add the SDK and async runtime baseline

- [ ] Add these dependencies to `Cargo.toml`:

```toml
rmcp = { version = "1.7.0", features = ["server", "transport-io", "macros"] }
schemars = "1"
tokio = { version = "1", features = ["full"] }
```

- [ ] Keep `ureq` initially, but call blocking provider operations through `tokio::task::spawn_blocking`.
- [ ] Run `cargo check` before changing tool routing.
- [ ] Run `cargo tree -i rmcp` and verify only `rmcp 1.7.0` is selected.
- [ ] Commit dependency and lockfile changes separately.

Acceptance:

- `cargo check` succeeds without changing the public tool contract.
- The default binary remains a single executable with no runtime service dependency.

## P0 Task 2 - Freeze the 33-tool public contract

- [ ] Treat `packaging/mcp/tools-list.snapshot.json` as the migration baseline.
- [ ] Add a test that asserts tool count, names, descriptions, required fields, defaults, enums, and top-level object schemas.
- [ ] Lock representative result payloads for these families:
  - session: `rlm_scan`, `rlm_session_list`, `rlm_session_export`
  - filter/map/reduce: `rlm_peek`, `rlm_chunk`, `rlm_map_plan`, `rlm_reduce_merge`
  - recursive tasks: `rlm_task_create`, `rlm_task_cancel`, `rlm_task_result`
  - budgets/trajectory: `rlm_budget_status`, `rlm_trajectory_get`
  - safety/REPL: `rlm_repl_info`, disabled `rlm_repl_execute`
  - benchmark/docs: `rlm_benchmark_list`, `rlm_tools_reference`, `rlm_workflow`
- [ ] Run these contract tests against the current custom server before adapter work.
- [ ] Prevent test code from automatically rewriting the snapshot during a normal test run.

Acceptance:

- Missing tools, schema drift, or response envelope changes fail CI.
- Migration can be reviewed family by family without losing tools.

## P0 Task 3 - Define typed Schemars inputs by tool family

- [x] Create a typed input struct for every one of the 33 tools.
- [x] Derive `Debug`, `Clone`, `Deserialize`, and `JsonSchema`.
- [x] Model closed values with enums:
  - benchmark suite and fixture size
  - budget mode
  - provider name
  - trajectory format
  - REPL backend
  - transform operation
- [x] Share nested types such as task budget without changing the JSON shape.
- [x] Encode defaults once so Serde runtime behavior and Schemars advertised values cannot diverge.
- [x] Preserve tools with empty object input rather than using null or an omitted schema.
- [x] Reject invalid types, unknown enum values, and missing required arguments as invalid params.
- [ ] Add parameter tests for minimum valid, complete valid, invalid enum, missing required, and boundary numeric values. (Partial: invalid enum and end-to-end valid calls are covered; add focused missing-required and numeric-boundary tests next.)

Acceptance:

- The generated schema matches `tools-list.snapshot.json` or an explicitly reviewed compatibility update.
- Hand-written schema construction is no longer required for any tool.

## P0 Task 4 - Build the typed rmcp router incrementally

- [x] Create a `#[tool_router]` implementation (implemented in `src/mcp/server.rs` to keep the server adapter compact).
- [x] Migrate tools in this order so every commit leaves a usable server:
  1. [x] workflow/reference/benchmark list tools
  2. [x] session lifecycle tools
  3. [x] peek/chunk/slice/transform/artifact tools
  4. [x] map/reduce coordination tools
  5. [x] recursive task and budget tools
  6. [x] trajectory, provider, REPL, and benchmark execution tools
- [x] Keep business logic in existing RLM modules and `ToolHandler`; router methods only validate, call, and convert results.
- [x] Centralize JSON text-content conversion to preserve current responses.
- [x] Prevent panics from escaping the tool boundary.
- [x] Make shared state concurrency explicit with `Arc` and existing per-session locking.

Acceptance:

- Every migrated family passes contract and end-to-end tests before the next family starts.
- All 33 tools are produced by the official router at the end of this task.

## P0 Task 5 - Implement ServerHandler and strict capability negotiation

- [ ] Implement official `ServerHandler` metadata with:
  - name: `rlm-mcp`
  - version: `env!("CARGO_PKG_VERSION")`
  - concise instructions for load, filter, map, reduce, and recursive follow-up
- [ ] Enable tools through the SDK capability builder.
- [ ] Set `listChanged` only if tool registration can change at runtime and notifications are implemented.
- [ ] Remove the hard-coded protocol version and let `rmcp` negotiate supported versions.
- [ ] Do not advertise resources, prompts, completion, logging, or subscriptions until implemented.
- [ ] Test initialization with the newest SDK-supported protocol and an accepted older client version.
- [ ] Test that client-declared capabilities do not cause unsupported server capabilities to appear.

Acceptance:

- An official `rmcp` client can initialize and list all 33 tools.
- Negotiation and capability fields are SDK-generated and spec-compliant.

## P0 Task 6 - Replace custom stdio and integrate Tokio lifecycle

- [ ] Make MCP execution use `#[tokio::main]` and official stdio transport.
- [ ] Preserve CLI subcommands and `--json --quiet` behavior outside MCP mode.
- [ ] Ensure CLI output and MCP output paths cannot accidentally share stdout logging.
- [ ] Treat stdin EOF as clean MCP shutdown.
- [ ] Flush/persist pending session and trajectory state before process exit where required.
- [ ] Move blocking filesystem scans, provider calls, command sandbox waits, and large serialization work off Tokio core workers.
- [ ] Add a process test proving EOF termination and parseable stdout frames.

Acceptance:

- OpenCode and Codex connect over stdio.
- CLI mode retains its existing contract.
- The server exits without hanging provider, sandbox, or session tasks.

## P0 Task 7 - Define protocol errors versus tool execution errors

- [ ] Use invalid params for malformed typed input.
- [ ] Let the SDK handle parse errors, unknown protocol methods, request IDs, and response envelopes.
- [ ] Return unknown tool names using the SDK's tool-call error behavior.
- [ ] Return valid-request domain failures as tool execution errors with `isError: true`, including:
  - missing or expired session
  - unknown chunk/task/artifact
  - path or output limit violation
  - disabled provider/network access
  - disabled REPL execution
  - budget exceeded
  - provider or sandbox failure
- [ ] Redact secrets, authorization headers, provider payloads, and local sensitive content from client-facing internal errors.
- [ ] Add tests proving one failed tool call does not terminate or poison the session.

Acceptance:

- Protocol and domain failures are distinguishable and stable.
- Error responses never expose provider credentials or unredacted trajectory secrets.

## P0 Task 8 - MCP cancellation and recursive task cancellation

- [x] Connect the SDK request cancellation signal to active router tool calls; deeper provider/REPL subprocess termination remains below.
- [x] Keep `rlm_task_cancel` as the domain-level persistent task-tree cancellation API.
- [x] Define precedence: MCP request cancellation stops the active request; `rlm_task_cancel` marks the persistent task tree cancelled.
- [ ] Ensure command providers and REPL subprocesses are terminated when cancellation is supported. (Follow-up: router-level request cancellation is bounded; blocking subprocess termination still needs provider/REPL token plumbing.)
- [ ] Do not persist a successful completion event after cancellation. (Follow-up for domain/provider cancellation paths.)
- [ ] Record a redacted cancellation trajectory event when a session/task exists. (Follow-up for session/task-aware cancellation paths.)
- [ ] Add timeout-bounded tests using mock providers and a slow command fixture. (Partial: official rmcp cancellable request path is covered; add slow command/provider fixture when provider/REPL token plumbing lands.)

Acceptance:

- Cancelled requests stop within a documented bound.
- Persistent session/task files remain readable and internally consistent.
- Later requests can inspect or resume unaffected sessions.

## P0 Task 9 - Preserve storage and concurrency behavior

- [ ] Run existing session storage, import/export, corrupt recovery, cleanup, and concurrent-reader tests unchanged before migration.
- [ ] Verify `Arc`/locking introduced by the async adapter does not hold a lock across `.await`.
- [ ] Keep atomic temp-file plus rename writes on Windows.
- [ ] Test simultaneous `rlm_chunk` readers while map completion, trajectory append, cleanup, and session delete occur.
- [ ] Add an MCP process restart test proving sessions created before migration remain readable.

Acceptance:

- No session format migration is required for the SDK change.
- Async routing introduces no deadlock, partial file, or lost trajectory event.

## P0 Task 10 - Remove manual JSON-RPC and schema code

- [ ] Delete `src/mcp/transport.rs` after Windows and Unix stdio tests pass.
- [ ] Remove manual initialize, ping, tools/list, tools/call, response formatting, and JSON-RPC error formatting from `src/mcp/server.rs`.
- [x] Remove `tool_definitions()` and all hand-built input schema JSON from `src/mcp/tools.rs` after snapshot parity.
- [x] Keep `src/mcp/schema_docs.rs` only for human/agent documentation that is verified against generated contracts.
- [ ] Run `rg` for `Content-Length`, `protocolVersion`, `format_response`, `format_error`, and `tool_definitions`; remaining matches must be tests or migration documentation.

Acceptance:

- `rmcp` is the only MCP protocol implementation.
- The RLM code owns domain behavior, not JSON-RPC mechanics.

## P0 Task 11 - Release artifact and installed-client verification

- [ ] Update `tests/release_smoke.rs` to launch the extracted `rlm-mcp`/`rlm-mcp.exe` over stdio.
- [ ] Verify initialize, tools/list, `rlm_scan`, `rlm_peek`, and `rlm_chunk` using a temporary cache/home.
- [ ] Verify one invalid-params response and one domain tool error from the extracted binary.
- [ ] Verify install dry-run writes stable paths rather than source `target/release` paths.
- [ ] Test generated OpenCode JSONC, Codex TOML, and generic `mcpServers` configuration.
- [ ] Verify installation and first use require no Rust toolchain and no rebuild.

Acceptance:

- Published archives work as MCP servers on fresh machines.
- OpenCode and Codex connect to the installed binary path.

## P1 Task 12 - Optional resources for external context

- [ ] Do not enable resources in the first migration release.
- [ ] Write a security/design decision before exposing session data as resources.
- [ ] If approved, limit resource URI templates to session metadata, chunk metadata, and explicitly selected artifacts.
- [ ] Enforce session access boundaries, redaction, byte limits, and expiry behavior.
- [ ] Never expose arbitrary filesystem reads or provider secrets through resources.
- [ ] Add list/read/template and negative security tests before advertising the capability.

Acceptance:

- Resources remain absent until their access and privacy model is proven.
- Tool-only clients remain fully functional.

## P1 Task 13 - Notifications, progress, and observability

- [ ] Evaluate SDK progress notifications for scans, benchmarks, and recursive task trees.
- [ ] Enable progress only when the client requested/supports it and tests cover the behavior.
- [ ] Keep progress diagnostics bounded and free of loaded context content by default.
- [ ] Do not set tool/resource list change capabilities merely to report task progress.
- [ ] Add tracing spans for request ID, tool name, duration, cancellation, and error category without logging secrets.

Acceptance:

- Clients that do not support progress see no protocol noise.
- Logs can diagnose latency and failures without leaking session contents.

## P1 Task 14 - Documentation and paper mapping

- [ ] Update `README.md` to state that transport/protocol compliance comes from the official Rust MCP SDK.
- [ ] Update `docs/tools.md` from or against generated tool metadata.
- [ ] Update `docs/paper-mapping.md` only where the asynchronous MCP boundary changes implementation mapping.
- [ ] Document stdio, stderr-only logs, session cache location, opt-in network/REPL behavior, cancellation, and shutdown.
- [ ] Add troubleshooting for stale installed paths, client timeout, invalid JSONC/TOML, disabled provider, and stdout pollution.
- [ ] Document that `cbm-mcp` is optional and independent.

Acceptance:

- Humans and agents can install, operate, and diagnose RLM MCP without reading source.
- Documentation does not imply that `rmcp` implements the RLM algorithm itself.

## Required verification before completion

- [ ] `cargo fmt --check`
- [ ] `cargo test --all-targets`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo build --release`
- [ ] `cargo tree -i rmcp` shows `rmcp 1.7.0`
- [ ] `cargo test --test rmcp_protocol`
- [ ] `cargo test --test mcp_contract`
- [ ] `cargo test --test cli_contract`
- [ ] `cargo test --test rlm_e2e`
- [ ] `cargo test --test session_storage`
- [ ] `cargo test --test release_smoke`
- [ ] Release binary initialize/tools-list/scan/peek/chunk smoke
- [ ] Fresh OpenCode connection smoke using the installed binary
- [ ] Fresh Codex connection smoke using the installed binary
- [ ] No network access occurs in default test or default local workflow
- [ ] `git status --short` contains no generated sessions, local configs, credentials, or `terminals/` changes

## Completion definition

The migration is complete only when the custom transport and manual protocol dispatcher/schema builder are gone, all 33 tools retain their locked contracts, official negotiation and error handling pass conformance tests, cancellation safely bounds long-running work, persisted sessions remain compatible, and release-installed binaries connect from OpenCode and Codex without recompilation.
