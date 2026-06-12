"""Configuration and binary discovery."""

from __future__ import annotations

import os
import sys
from pathlib import Path


def resolve_cbm_binary() -> list[str]:
    """Resolve codebase-memory-mcp launch command."""
    if env := os.environ.get("CBM_BINARY"):
        return [env]

    if env_cmd := os.environ.get("CBM_COMMAND"):
        # Space-separated command, e.g. "pwsh -NoProfile -Command & ..."
        return env_cmd.split()

    home = Path.home()
    candidates: list[Path] = []

    if sys.platform == "win32":
        candidates.extend([
            Path(r"D:\cbm-mcp\target\release\codebase-memory-mcp.exe"),
            home / ".config" / "codebase-memory-mcp" / "bin" / "codebase-memory-mcp.exe",
            home / ".config" / "opencode-codebase-memory-mcp" / "bin" / "codebase-memory-mcp.exe",
            Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "codebase-memory-mcp" / "codebase-memory-mcp.exe",
        ])
    else:
        candidates.extend([
            home / ".local" / "bin" / "codebase-memory-mcp",
            home / ".config" / "codebase-memory-mcp" / "bin" / "codebase-memory-mcp",
        ])

    for path in candidates:
        if path.is_file():
            return [str(path)]

    return ["codebase-memory-mcp"]


def default_project() -> str | None:
    return os.environ.get("CBM_PROJECT")