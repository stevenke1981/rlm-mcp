# rlm-mcp Security Guide

## Threat Model

rlm-mcp is an MCP server that processes long text via external sessions. The main
security considerations are:

1. **Command Provider** (`RLM_PROVIDER_COMMAND`): executes arbitrary executables
   as sub-task workers. This is the highest-risk feature.
2. **OpenAI Provider** (`RLM_OPENAI_API_KEY`): makes network requests with an API key.
3. **Path traversal**: session scan paths must be validated to prevent accessing
   unintended files.
4. **Session data**: session content is stored on disk in `RLM_CACHE_DIR`.
5. **Secrets exposure**: API keys and sensitive data must never be persisted in
   sessions or logs.

---

## 1. Command Provider Sandbox

The `command` provider allows the MCP server to execute external programs for
sub-task processing. Starting in v0.1.7, a **provider sandbox** provides three
security modes controlled by the `RLM_PROVIDER_SANDBOX` environment variable:

| Mode | Behavior |
|------|----------|
| `warn` (default) | Logs a warning for every command invocation. Backward compatible. |
| `strict` | Validates the command path against allowed directories; rejects shell interpreters (`sh`, `bash`, `cmd.exe`, `powershell`, etc.). |
| `off` | No sandbox restrictions. |

### Strict Mode

When `RLM_PROVIDER_SANDBOX=strict`:

- **Shell interpreters are rejected.** Commands like `sh`, `bash`, `cmd.exe`,
  `powershell`, `python`, `node` are blocked even when referenced by bare name.
- **Absolute paths required.** Bare command names are accepted only if they are
  not shell interpreters. Resolved paths are checked against allowed directories.
- **Allowed directories.** Set `RLM_PROVIDER_ALLOWED_DIRS` to a
  semicolon-separated list of absolute directory paths:
  ```powershell
  $env:RLM_PROVIDER_ALLOWED_DIRS = "C:\tools\my-scripts;D:\work\analysis"
  ```
  The command's parent directory must start with one of the allowed paths.

### Example: Safe command provider setup

```powershell
# Allow only a specific analysis script
$env:RLM_PROVIDER_COMMAND = "C:\tools\analysis\summarize.exe"
$env:RLM_PROVIDER_SANDBOX = "strict"
$env:RLM_PROVIDER_ALLOWED_DIRS = "C:\tools"
rlm-mcp
```

### Risk Mitigation

Even in `off` mode, the command provider:
- Runs with the same user permissions as the rlm-mcp process.
- Receives input via stdin and environment variables (`RLM_SUB_PROMPT`,
  `RLM_SUB_CONTEXT`), not CLI arguments.
- Output is captured from stdout.
- Is killed if the MCP request is cancelled or if
  `RLM_PROVIDER_MAX_WALL_SECS` (default `300`) elapses.

For production deployments:
- Run rlm-mcp in a container (see [Docker support](#docker)).
- Use a dedicated non-privileged user account.
- Set `RLM_PROVIDER_SANDBOX=strict` and pin allowed directories.

---

## 2. Network Security

The OpenAI-compatible provider requires explicit opt-in:

```powershell
$env:RLM_ALLOW_NETWORK = "1"
$env:RLM_OPENAI_API_KEY = "sk-..."
$env:RLM_OPENAI_BASE_URL = "https://api.openai.com/v1"
```

- Network is **disabled by default** (offline-safe).
- API keys are read from environment variables **only** — never stored in
  sessions or persisted to disk.
- Response metadata is automatically sanitized to strip `api_key`,
  `authorization`, and `secret` fields from structured output.

---

## 3. Path Traversal Protection

All `rlm_scan` paths are validated:

- Paths containing `..` segments are rejected.
- Paths are canonicalized via `std::fs::canonicalize` before use.
- Binary files (containing NUL bytes or high control-character ratio) are
  detected and skipped.

---

## 4. Session Data Security

- Session files are stored under `RLM_CACHE_DIR/rlm-sessions/` in JSON format.
- Session files have a configurable TTL (`RLM_SESSION_TTL_SECS`, default 3600s).
- Expired sessions are automatically cleaned up.
- API keys and secrets are never written to session files.
- The `redact_secrets` utility strips known secret patterns (`sk-`, `Bearer `,
  `api_key=`, `password=`, etc.) from trajectory and log output.

---

## 5. Budget & Resource Limits

Resource limits prevent unbounded resource consumption:

| Variable | Default | Purpose |
|----------|---------|---------|
| `RLM_MAX_FILE_BYTES` | 512 KB | Max single file |
| `RLM_MAX_TOTAL_BYTES` | 8 MB | Max total session |
| `RLM_MAX_CHUNKS` | 10,000 | Max chunks per session |
| `RLM_MAX_SESSIONS` | 50 | Max persisted sessions |
| `RLM_SESSION_TTL_SECS` | 3600 | Session expiry |
| `RLM_MAX_CHUNK_BYTES` | 256 KB | Max bytes per chunk output |
| `RLM_MAX_ARTIFACT_BYTES` | 1 MB | Max artifact size |
| `RLM_MAX_TRANSFORM_BYTES` | 512 KB | Max transform input |

Budget ceilings prevent runaway recursion:

| Budget parameter | Default | Purpose |
|----------------|---------|---------|
| `rlm_budget_configure` — `max_chunks_read` | 500 | Chunk read limit per session |
| `rlm_budget_configure` — `max_sub_calls` | 64 | Sub-call limit per session |
| `rlm_budget_configure` — `max_total_tokens_est` | 500K | Estimated token limit |
| `rlm_budget_configure` — `max_wall_secs` | 600 | Wall-clock limit |

---

## 6. Docker Isolation

For the strongest isolation, run rlm-mcp in a container (see the
[`Dockerfile`](../Dockerfile) and [`docker-compose.yml`](../docker-compose.yml)).

Container deployments:
- Can mount a read-only host directory for scan sources.
- Can limit CPU and memory via Docker Compose resource constraints.
- Can inject API keys as environment variables without exposing to the host.

---

## 7. Reporting Vulnerabilities

If you discover a security issue, please open a GitHub Issue or contact the
maintainer directly. Do not post exploit details publicly until a fix is
available.
