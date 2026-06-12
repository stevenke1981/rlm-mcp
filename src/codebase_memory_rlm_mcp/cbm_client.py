"""MCP client wrapper for codebase-memory-mcp."""

from __future__ import annotations

import json
from typing import Any

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

from .config import resolve_cbm_binary


def _extract_text(result: Any) -> str:
    parts: list[str] = []
    for item in getattr(result, "content", []) or []:
        text = getattr(item, "text", None)
        if text:
            parts.append(text)
    return "\n".join(parts) if parts else json.dumps({"raw": str(result)})


class CBMClient:
    def __init__(self, command: list[str] | None = None) -> None:
        self.command = command or resolve_cbm_binary()

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> str:
        if not self.command:
            raise RuntimeError("codebase-memory-mcp binary not configured")

        params = StdioServerParameters(
            command=self.command[0],
            args=self.command[1:] if len(self.command) > 1 else None,
        )

        async with stdio_client(params) as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                result = await session.call_tool(name, arguments)
                return _extract_text(result)

    async def search_graph(
        self,
        project: str,
        query: str | None = None,
        name_pattern: str | None = None,
        label: str | None = None,
        limit: int = 20,
    ) -> str:
        args: dict[str, Any] = {"project": project, "limit": limit}
        if query:
            args["query"] = query
        if name_pattern:
            args["name_pattern"] = name_pattern
        if label:
            args["label"] = label
        return await self.call_tool("search_graph", args)

    async def search_code_files(
        self,
        project: str,
        pattern: str,
        file_pattern: str | None = None,
        limit: int = 50,
    ) -> str:
        args: dict[str, Any] = {
            "project": project,
            "pattern": pattern,
            "mode": "files",
            "limit": limit,
        }
        if file_pattern:
            args["file_pattern"] = file_pattern
        return await self.call_tool("search_code", args)

    async def get_code_snippet(self, project: str, qualified_name: str) -> str:
        return await self.call_tool(
            "get_code_snippet",
            {"project": project, "qualified_name": qualified_name},
        )

    async def trace_path(
        self,
        project: str,
        function_name: str,
        direction: str = "both",
        depth: int = 3,
        mode: str = "calls",
    ) -> str:
        return await self.call_tool(
            "trace_path",
            {
                "project": project,
                "function_name": function_name,
                "direction": direction,
                "depth": depth,
                "mode": mode,
            },
        )

    async def get_architecture(self, project: str) -> str:
        return await self.call_tool("get_architecture", {"project": project})

    async def index_status(self, project: str) -> str:
        return await self.call_tool("index_status", {"project": project})

    async def detect_changes(self, project: str, scope: str | None = None) -> str:
        args: dict[str, Any] = {"project": project}
        if scope:
            args["scope"] = scope
        return await self.call_tool("detect_changes", args)