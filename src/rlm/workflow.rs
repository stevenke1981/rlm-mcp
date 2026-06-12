use serde_json::{json, Value};

pub fn workflow_guidance(phase: &str) -> Value {
    match phase {
        "filter" => json!({
            "phase": "filter",
            "description": "RLM Phase 1 — narrow context via graph search",
            "tools": ["rlm_filter", "rlm_index_status", "search_graph", "search_code"],
            "steps": [
                "index_repository via codebase-memory-mcp if not indexed",
                "rlm_index_status then rlm_filter (query/label or pattern)",
                "collect qualified_names for map phase"
            ],
            "rules": [
                "Never load 10+ files into root context",
                "Prefer graph tools over rg when indexed"
            ]
        }),
        "map" => json!({
            "phase": "map",
            "description": "RLM Phase 2 — parallel symbol reads",
            "tools": ["rlm_read_symbol", "rlm_trace", "get_code_snippet", "rlm_chunk"],
            "steps": [
                "rlm_read_symbol — one qualified_name per call",
                "rlm_trace for call chains (direction=both, depth=3)",
                "rlm_chunk for log/CSV blobs (non-code only)"
            ],
            "rules": [
                "One symbol per rlm_read_symbol call",
                "Use rlm_trace before reading unrelated files"
            ]
        }),
        "reduce" => json!({
            "phase": "reduce",
            "description": "RLM Phase 3 — synthesize architecture summary",
            "tools": ["rlm_architecture", "rlm_detect_changes", "get_architecture"],
            "steps": [
                "rlm_architecture for project overview",
                "rlm_detect_changes for git delta",
                "Reduce to structured JSON before final answer"
            ]
        }),
        _ => json!({
            "phase": "overview",
            "description": "Recursive Language Model workflow for large codebases",
            "paper": "https://arxiv.org/pdf/2512.24601",
            "phases": ["filter", "map", "reduce"],
            "prerequisite": "index_repository via codebase-memory-mcp",
            "project_naming": "CBM indexes use cbm+ prefix (set CBM_PROJECT or pass project)",
            "requires": "codebase-memory-mcp binary (CBM_BINARY or PATH)",
            "loop": "filter → map (parallel) → reduce"
        }),
    }
}