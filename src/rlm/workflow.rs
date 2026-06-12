use serde_json::{json, Value};

pub fn workflow_guidance(phase: &str) -> Value {
    match phase {
        "load" => json!({
            "phase": "load",
            "description": "RLM Phase 1 — load external context into a session",
            "tools": ["rlm_scan", "rlm_session_list"],
            "steps": [
                "rlm_scan(path) — load files/logs/docs into session (returns session_id + metadata)",
                "Inspect file_count, chunk_count, total_bytes before proceeding"
            ],
            "rules": [
                "Never paste full file contents into root context",
                "Context lives in the RLM session until deleted"
            ]
        }),
        "filter" => json!({
            "phase": "filter",
            "description": "RLM Phase 2 — narrow candidates without loading everything",
            "tools": ["rlm_peek", "rlm_chunk"],
            "steps": [
                "rlm_peek(session_id, query) — substring/path search across chunks",
                "Note matching paths and chunk offsets for map phase"
            ],
            "rules": [
                "Filter before reading large chunk ranges",
                "Use specific queries (error codes, function names, log markers)"
            ]
        }),
        "map" => json!({
            "phase": "map",
            "description": "RLM Phase 3 — read and process chunks in parallel",
            "tools": ["rlm_chunk"],
            "steps": [
                "rlm_chunk(session_id, file_pattern?, offset, limit) — paginated reads",
                "Spawn parallel workers: one chunk batch per sub-task",
                "Each worker returns structured JSON (findings, not raw dumps)"
            ],
            "rules": [
                "One focused sub-task per worker",
                "Keep limit small (3–5 chunks per call)"
            ]
        }),
        "reduce" => json!({
            "phase": "reduce",
            "description": "RLM Phase 4 — synthesize final answer from worker outputs",
            "tools": ["rlm_peek", "rlm_chunk"],
            "steps": [
                "Merge worker JSON into a single structured result",
                "If gaps remain, run another filter → map pass on missing areas only",
                "Produce final answer only after reduce"
            ],
            "rules": [
                "Do not re-read entire session at reduce time",
                "Second recursion only for proven gaps"
            ]
        }),
        _ => json!({
            "phase": "overview",
            "description": "Recursive Language Model — external context, programmatic access",
            "paper": "https://arxiv.org/pdf/2512.24601",
            "phases": ["load", "filter", "map", "reduce"],
            "loop": "load → filter → map (parallel) → reduce",
            "standalone": true,
            "core_tools": [
                "rlm_workflow", "rlm_scan", "rlm_env_info", "rlm_peek", "rlm_slice",
                "rlm_transform", "rlm_repl_info", "rlm_repl_execute",
                "rlm_artifact_write", "rlm_artifact_read",
                "rlm_chunk", "rlm_map_plan", "rlm_map_claim", "rlm_map_complete",
                "rlm_reduce_schema", "rlm_reduce_merge",
                "rlm_task_create", "rlm_task_list", "rlm_task_result", "rlm_task_reduce",
                "rlm_trajectory_get", "rlm_trajectory_final",
                "rlm_budget_configure", "rlm_budget_status", "rlm_task_cancel",
                "rlm_session_list", "rlm_session_delete"
            ],
            "principle": "Context is external. LLM orchestrates via MCP tools — no bulk context loading."
        }),
    }
}
