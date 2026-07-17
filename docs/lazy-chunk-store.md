# Lazy on-disk chunk store

## Problem

Streaming path scans still left full chunk bodies inside `ScanSession` and the
session JSON under `rlm-sessions/`. Peak memory and disk I/O scaled with the
**whole corpus**, not with “what the agent actually reads”.

## Design

| Layer | What is stored |
|-------|----------------|
| Session JSON (`rlm-sessions/<id>.json`) | Metadata only: chunk id, path, offset, line_count, optional `content_file` |
| Chunk store (`rlm-chunks/<session_id>/<chunk_id>.txt`) | UTF-8 body for each chunk |
| In-memory after scan | Same as session JSON (empty `content` when lazy) |

### Lifecycle

1. **Path scan** — pre-allocate `session_id`; as each line-window closes, write
   body to `rlm-chunks/...` immediately; keep only metadata in the `chunks` vec.
2. **Text scan** — build inline chunks, then `spill_session_chunks` before persist.
3. **Read path** (`peek` / `chunk` / `slice` / tasks / transform) — resolve via
   `chunk_store::resolve_content` (inline legacy or disk file).
4. **Export** — materialize bodies into JSON for a portable self-contained session.
5. **Import** — spill imported bodies back to disk.
6. **Delete** — remove session JSON and the session’s chunk directory.

### Backward compatibility

Older sessions with full inline `content` and no `content_file` still load and
resolve correctly.

### What this is not

- Not memory-mapped multi-GB single files beyond `RLM_MAX_FILE_BYTES`.
- Not a vector/semantic index (see `docs/embedding-roadmap.md`).
- Agent tools still receive content only when they call `rlm_chunk` / peek with
  `include_content` — the model context is not auto-stuffed.
