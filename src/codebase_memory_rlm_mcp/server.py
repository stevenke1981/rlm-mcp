"""MCP server: RLM orchestration + codebase-memory-mcp integration."""

from __future__ import annotations

import json

from mcp.server.fastmcp import FastMCP

from .cbm_client import CBMClient
from .config import default_project
from .rlm_engine import SessionStore

mcp = FastMCP("codebase-memory-rlm-mcp")
_sessions = SessionStore()
_cbm = CBMClient()

WORKFLOW_GUIDE = {
    "overview": (
        "RLM + codebase-memory-mcp integration. "
        "Requires codebase-memory-mcp installed and indexed project. "
        "Loop: index_status → rlm_filter → parallel rlm_read_symbol/rlm_trace → reduce."
    ),
    "filter": (
        "Phase 1 Filter: call rlm_index_status, then rlm_filter (graph search). "
        "For logs/CSV use rlm_scan + rlm_peek. Do NOT bulk-read files into context."
    ),
    "map": (
        "Phase 2 Map: spawn parallel workers. Each worker calls rlm_read_symbol (one qn) "
        "or rlm_trace (one function). For huge files use rlm_chunk with session_id."
    ),
    "reduce": (
        "Phase 3 Reduce: merge worker JSON outputs. Use rlm_trace or rlm_detect_changes "
        "for gaps. Run second recursion only on missing symbols."
    ),
}


def _project(project: str | None) -> str:
    resolved = project or default_project()
    if not resolved:
        raise ValueError("project is required (or set CBM_PROJECT env var)")
    return resolved


@mcp.tool()
async def rlm_workflow(phase: str = "overview") -> str:
    """Return RLM loop guidance for the given phase: overview, filter, map, or reduce."""
    guide = WORKFLOW_GUIDE.get(phase, WORKFLOW_GUIDE["overview"])
    return json.dumps({"phase": phase, "guide": guide}, indent=2)


@mcp.tool()
async def rlm_index_status(project: str | None = None) -> str:
    """Check codebase-memory-mcp index status for a project."""
    return await _cbm.index_status(_project(project))


@mcp.tool()
async def rlm_filter(
    project: str | None = None,
    query: str | None = None,
    pattern: str | None = None,
    label: str | None = None,
    limit: int = 20,
) -> str:
    """Filter candidates without loading full files. Uses search_graph (query/label) or search_code files mode (pattern)."""
    proj = _project(project)
    if query or label:
        return await _cbm.search_graph(proj, query=query, label=label, limit=limit)
    if pattern:
        return await _cbm.search_code_files(proj, pattern, limit=limit)
    raise ValueError("Provide query/label (graph search) or pattern (file path search)")


@mcp.tool()
async def rlm_read_symbol(project: str | None = None, qualified_name: str = "") -> str:
    """Read one symbol for Map phase. Wraps get_code_snippet — one qualified_name per call."""
    if not qualified_name:
        raise ValueError("qualified_name is required")
    return await _cbm.get_code_snippet(_project(project), qualified_name)


@mcp.tool()
async def rlm_trace(
    project: str | None = None,
    function_name: str = "",
    direction: str = "both",
    depth: int = 3,
    mode: str = "calls",
) -> str:
    """Trace call/data-flow paths. Wraps trace_path for impact analysis during Reduce."""
    if not function_name:
        raise ValueError("function_name is required")
    return await _cbm.trace_path(
        _project(project), function_name, direction=direction, depth=depth, mode=mode
    )


@mcp.tool()
async def rlm_architecture(project: str | None = None) -> str:
    """High-level architecture overview from codebase-memory-mcp graph."""
    return await _cbm.get_architecture(_project(project))


@mcp.tool()
async def rlm_detect_changes(project: str | None = None, scope: str | None = None) -> str:
    """Detect git changes and impact via codebase-memory-mcp."""
    return await _cbm.detect_changes(_project(project), scope=scope)


@mcp.tool()
def rlm_scan(path: str = ".", chunk_size: int = 5000) -> str:
    """Scan directory into an RLM session (for logs/CSV/non-graph files). Returns session_id."""
    session_id, summary = _sessions.create(path, chunk_size=chunk_size)
    return json.dumps({"session_id": session_id, **summary}, indent=2)


@mcp.tool()
def rlm_peek(session_id: str, query: str, limit: int = 20) -> str:
    """Peek query snippets inside an RLM session without loading full files."""
    ctx = _sessions.get(session_id)
    results = ctx.peek(query, limit=limit)
    return json.dumps({"session_id": session_id, "matches": results}, indent=2)


@mcp.tool()
def rlm_chunk(
    session_id: str,
    file_pattern: str | None = None,
    offset: int = 0,
    limit: int = 5,
) -> str:
    """Get paginated chunks from an RLM session. Use for parallel Map on huge files."""
    ctx = _sessions.get(session_id)
    result = ctx.chunks(file_pattern=file_pattern, offset=offset, limit=limit)
    result["session_id"] = session_id
    return json.dumps(result, indent=2)


@mcp.tool()
def rlm_session_list() -> str:
    """List active RLM scan sessions."""
    return json.dumps({"sessions": _sessions.list_sessions()}, indent=2)


@mcp.tool()
def rlm_session_delete(session_id: str) -> str:
    """Delete an RLM session and free memory."""
    deleted = _sessions.delete(session_id)
    return json.dumps({"session_id": session_id, "deleted": deleted})


def main() -> None:
    mcp.run()


if __name__ == "__main__":
    main()