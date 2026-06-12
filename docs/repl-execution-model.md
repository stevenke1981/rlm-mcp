# REPL execution model

This document records the P0 decision for how `rlm-mcp` implements the paper's REPL-style interaction without making the default MCP server unsafe.

## Decision (P0)

| Mode | Status | Description |
|------|--------|-------------|
| **Safe built-in transforms** | **Default (shipped)** | `rlm_transform` applies deterministic string ops. No arbitrary code. |
| **Derived artifacts** | **Default (shipped)** | `rlm_artifact_write` / `rlm_artifact_read` persist transform outputs under `RLM_CACHE_DIR/rlm-artifacts/<session_id>/`. |
| **Embedded scripting sandbox** | Deferred P2 | Optional backend trait; not enabled by default. |
| **External Python REPL** | Deferred P2 | Requires explicit env flag + documented limits. |

**Rationale:** MCP agents already orchestrate tools. Typed, bounded transforms give REPL-like snippet manipulation without shipping a Python interpreter in the default binary.

## Safe transform operations

`rlm_transform` supports:

- `dedupe_lines`, `sort_lines`, `filter_lines`
- `head_lines`, `tail_lines`, `truncate_chars`
- `add_line_numbers`, `count_lines`, `normalize_whitespace`

Input resolution (first match wins):

1. `content` (inline)
2. `artifact_name`
3. `chunk_id`

## Output limits (shipped)

| Env var | Default | Applies to |
|---------|---------|------------|
| `RLM_MAX_TRANSFORM_BYTES` | 262144 (256 KiB) | `rlm_transform` output |
| `RLM_MAX_ARTIFACT_BYTES` | same as transform max | `rlm_artifact_write` / read |

Oversized transform output is truncated with `truncated: true`. Oversized artifact writes are rejected.

## Executable REPL limits (P2 policy)

When optional sandboxes ship (P2), they must enforce:

| Limit | Planned default |
|-------|-----------------|
| Wall time | 30s (`RLM_REPL_MAX_WALL_SECS`) |
| Memory | 128 MiB (`RLM_REPL_MAX_MEMORY_MB`) |
| Output bytes | `RLM_MAX_TRANSFORM_BYTES` |
| Network | Off unless `RLM_ALLOW_NETWORK=1` |
| Filesystem | Session cache + artifact dir only |

Audit: trajectory events + optional JSONL export.

## Agent workflow

```
rlm_scan → rlm_peek → rlm_slice / rlm_chunk
    → rlm_transform → rlm_artifact_write
    → rlm_map_plan → rlm_reduce_merge
```

See [`docs/rlm-loop.md`](rlm-loop.md) and [`examples/parallel-workers.md`](../examples/parallel-workers.md).