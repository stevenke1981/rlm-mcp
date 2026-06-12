import tempfile
from pathlib import Path

from codebase_memory_rlm_mcp.rlm_engine import RLMContext, SessionStore


def test_load_and_peek():
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "a.log").write_text("ERROR: auth failed\nINFO: ok\n", encoding="utf-8")
        ctx = RLMContext(root=root)
        summary = ctx.load()
        assert summary["files_loaded"] == 1
        hits = ctx.peek("ERROR")
        assert len(hits) == 1
        assert "auth failed" in hits[0]


def test_chunk_pagination():
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "big.txt").write_text("x" * 12000, encoding="utf-8")
        ctx = RLMContext(root=root, chunk_size=5000)
        ctx.load()
        page1 = ctx.chunks(offset=0, limit=1)
        page2 = ctx.chunks(offset=1, limit=1)
        assert page1["total"] == 3
        assert len(page1["chunks"]) == 1
        assert len(page2["chunks"]) == 1


def test_session_store():
    with tempfile.TemporaryDirectory() as tmp:
        store = SessionStore()
        sid, summary = store.create(tmp)
        assert summary["files_loaded"] >= 0
        assert sid in [s["session_id"] for s in store.list_sessions()]
        assert store.delete(sid)