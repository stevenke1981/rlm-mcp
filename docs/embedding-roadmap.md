# Semantic embedding peek (deferred)

## Status

**Not implemented** in the binary. Ranking search today is **BM25**
(`rlm_peek` with `bm25=true`).

## Why deferred

| Concern | Detail |
|---------|--------|
| Binary size | ONNX / fastembed models add tens–hundreds of MB to a currently small static binary |
| Default safety | No network model download in default install path |
| Overlap with BM25 | Paper baselines already treat BM25 as the lightweight retrieval baseline |
| Ops cost | GPU optional, model versioning, cache under `RLM_CACHE_DIR` |

## Target design (when enabled)

1. Optional Cargo feature, e.g. `semantic-peek`, default **off**.
2. Env gate: `RLM_SEMANTIC_PEEK=1` plus local model path `RLM_EMBED_MODEL_PATH`.
3. Index artifacts under `rlm-chunks/<session>/embeddings.bin` (or similar),
   built lazily on first semantic peek.
4. New peek mode flag: `semantic=true` mutually exclusive with `regex`;
   can combine with path/glob filters like BM25.
5. Never send corpus text to a remote embedding API unless
   `RLM_ALLOW_NETWORK=1` and an explicit remote embed endpoint is set.

## Acceptance criteria (future PR)

- Feature-off builds unchanged in size and tools snapshot.
- Feature-on: mini fixture ranks a buried needle line above filler.
- Document model license and download steps in README.
- CI job for feature-on is optional/nightly only.

## Agent guidance until then

Use `bm25=true` for “most relevant” lines; use substring/regex for exact IDs.
