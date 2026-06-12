"""Session-based RLM engine for dense file chunking."""

from __future__ import annotations

import fnmatch
import glob
import math
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

SKIP_DIRS = {".git", "__pycache__", "node_modules", ".venv", "venv", "dist", "build"}


@dataclass
class RLMContext:
    root: Path
    index: Dict[str, str] = field(default_factory=dict)
    chunk_size: int = 5000

    def load(self, pattern: str = "**/*") -> dict[str, Any]:
        files = glob.glob(str(self.root / pattern), recursive=True)
        loaded = 0
        self.index.clear()
        for f in files:
            path = Path(f)
            if not path.is_file():
                continue
            if any(part in SKIP_DIRS for part in path.parts):
                continue
            try:
                self.index[str(path)] = path.read_text(encoding="utf-8", errors="ignore")
                loaded += 1
            except OSError:
                pass
        total_chars = sum(len(c) for c in self.index.values())
        return {
            "files_loaded": loaded,
            "total_chars": total_chars,
            "root": str(self.root),
        }

    def peek(self, query: str, context_window: int = 200, limit: int = 20) -> list[str]:
        results: list[str] = []
        for path, content in self.index.items():
            if query not in content:
                continue
            start = 0
            while True:
                idx = content.find(query, start)
                if idx == -1:
                    break
                snippet_start = max(0, idx - context_window)
                snippet_end = min(len(content), idx + len(query) + context_window)
                snippet = content[snippet_start:snippet_end]
                results.append(f"[{path}]: ...{snippet}...")
                start = idx + 1
                if len(results) >= limit:
                    return results
        return results

    def chunks(
        self,
        file_pattern: Optional[str] = None,
        offset: int = 0,
        limit: int = 10,
    ) -> dict[str, Any]:
        all_chunks: list[dict[str, Any]] = []
        targets = list(self.index.keys())

        if file_pattern:
            targets = [
                f for f in targets
                if fnmatch.fnmatch(Path(f).name, file_pattern)
                or fnmatch.fnmatch(f, file_pattern)
                or file_pattern in f
            ]

        for path in targets:
            content = self.index[path]
            total = math.ceil(len(content) / self.chunk_size) or 1
            for i in range(total):
                start = i * self.chunk_size
                end = min((i + 1) * self.chunk_size, len(content))
                all_chunks.append({
                    "source": path,
                    "chunk_id": i,
                    "total_chunks": total,
                    "content": content[start:end],
                })

        page = all_chunks[offset: offset + limit]
        return {
            "total": len(all_chunks),
            "offset": offset,
            "limit": limit,
            "chunks": page,
        }


class SessionStore:
    def __init__(self) -> None:
        self._sessions: dict[str, RLMContext] = {}

    def create(self, path: str, chunk_size: int = 5000) -> tuple[str, dict[str, Any]]:
        session_id = uuid.uuid4().hex[:12]
        ctx = RLMContext(root=Path(path).resolve(), chunk_size=chunk_size)
        summary = ctx.load()
        self._sessions[session_id] = ctx
        return session_id, summary

    def get(self, session_id: str) -> RLMContext:
        if session_id not in self._sessions:
            raise KeyError(f"Unknown session: {session_id}")
        return self._sessions[session_id]

    def list_sessions(self) -> list[dict[str, Any]]:
        return [
            {
                "session_id": sid,
                "root": str(ctx.root),
                "files_loaded": len(ctx.index),
                "total_chars": sum(len(c) for c in ctx.index.values()),
            }
            for sid, ctx in self._sessions.items()
        ]

    def delete(self, session_id: str) -> bool:
        return self._sessions.pop(session_id, None) is not None