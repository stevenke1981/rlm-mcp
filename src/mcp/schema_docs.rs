use serde_json::{json, Value};

/// Structured schema reference for agent integration (MCP + CLI).
pub fn tools_reference() -> Value {
    let mut entries: Vec<Value> = crate::mcp::server::McpServer::rmcp_tool_definitions()
        .into_iter()
        .map(|tool| serde_json::to_value(tool).expect("rmcp tool definition must serialize"))
        .map(|value| enrich_tool(&value))
        .collect();
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    json!({
        "server": crate::mcp::server::SERVER_NAME,
        "version": crate::mcp::server::SERVER_VERSION,
        "tool_count": entries.len(),
        "cli_flag_style": "--kebab-case",
        "json_output": "Pass --json or --quiet on CLI subcommands for parseable stdout",
        "tools": entries
    })
}

fn enrich_tool(def: &Value) -> Value {
    let name = def["name"].as_str().unwrap_or("");
    let (cli_command, cli_flags, returns, example_mcp, example_cli) = tool_meta(name);
    json!({
        "name": name,
        "phase": phase_for(name),
        "description": def["description"],
        "input_schema": def["inputSchema"],
        "cli": {
            "command": cli_command,
            "flags": cli_flags,
            "json_flags": ["--json", "--quiet"]
        },
        "returns": returns,
        "example_mcp_arguments": example_mcp,
        "example_cli": example_cli
    })
}

fn phase_for(name: &str) -> &'static str {
    match name {
        "rlm_scan"
        | "rlm_env_info"
        | "rlm_session_list"
        | "rlm_session_delete"
        | "rlm_session_cleanup"
        | "rlm_session_export"
        | "rlm_session_import" => "load",
        "rlm_peek" | "rlm_slice" | "rlm_transform" | "rlm_repl_info" | "rlm_repl_execute"
        | "rlm_artifact_read" | "rlm_artifact_write" => "repl",
        "rlm_chunk" | "rlm_map_plan" | "rlm_map_claim" | "rlm_map_complete" => "map",
        "rlm_reduce_schema" | "rlm_reduce_merge" => "reduce",
        "rlm_task_create" | "rlm_task_list" | "rlm_task_result" | "rlm_task_reduce"
        | "rlm_task_cancel" => "recurse",
        "rlm_trajectory_get" | "rlm_trajectory_final" | "rlm_budget_status" => "observe",
        "rlm_budget_configure" => "control",
        "rlm_benchmark_list" | "rlm_benchmark_run" => "benchmark",
        "rlm_workflow" | "rlm_tools_reference" => "help",
        _ => "other",
    }
}

#[allow(clippy::type_complexity)]
fn tool_meta(
    name: &str,
) -> (
    &'static str,
    &'static [&'static str],
    &'static str,
    Value,
    &'static str,
) {
    match name {
        "rlm_workflow" => (
            "workflow",
            &["--phase"],
            "Phase guidance object (steps, tools, rules)",
            json!({ "phase": "filter" }),
            "rlm-mcp workflow --phase filter --json",
        ),
        "rlm_scan" => (
            "scan",
            &["--path", "--content", "--virtual-path", "--variable", "--stdin"],
            "session_id, chunk_count, total_bytes, metadata",
            json!({ "content": "log line\\n", "virtual_path": "app.log" }),
            "rlm-mcp scan --content \"log line\" --virtual-path app.log --json",
        ),
        "rlm_env_info" => (
            "env-info",
            &["--session-id"],
            "files, chunk_ids, context_length_bytes, expiry",
            json!({ "session_id": "<session_id>" }),
            "rlm-mcp env-info --session-id <id> --json",
        ),
        "rlm_peek" => (
            "peek",
            &[
                "--session-id",
                "--query",
                "--path",
                "--glob",
                "--regex",
                "--ignore-case",
                "--line-start",
                "--line-end",
                "--context",
                "--limit",
                "--full",
            ],
            "matches with chunk_id, line, preview; file_summary",
            json!({ "session_id": "<session_id>", "query": "ERROR", "limit": 10 }),
            "rlm-mcp peek --session-id <id> --query ERROR --limit 10 --json",
        ),
        "rlm_slice" => (
            "slice",
            &["--session-id", "--chunk-id", "--start", "--end"],
            "content slice for line range within one chunk",
            json!({ "session_id": "<session_id>", "chunk_id": "c-0", "start_line": 1, "end_line": 5 }),
            "rlm-mcp slice --session-id <id> --chunk-id c-0 --start 1 --end 5 --json",
        ),
        "rlm_transform" => (
            "transform",
            &["--session-id", "--op", "--chunk-id", "--artifact", "--content", "--params"],
            "transformed content with operation metadata",
            json!({ "session_id": "<session_id>", "operation": "dedupe_lines", "chunk_id": "c-0" }),
            "rlm-mcp transform --session-id <id> --chunk-id c-0 --op dedupe_lines --json",
        ),
        "rlm_repl_info" => (
            "repl-info",
            &[],
            "backends, capability flags, limits, opt-in env vars",
            json!({}),
            "rlm-mcp repl-info --json",
        ),
        "rlm_repl_execute" => (
            "repl-exec",
            &["--session-id", "--code", "--language", "--backend"],
            "sandbox output with audit metadata (opt-in executable backends)",
            json!({ "session_id": "<session_id>", "code": "print('hi')", "backend": "command" }),
            "rlm-mcp repl-exec --session-id <id> --code \"echo hi\" --backend command --json",
        ),
        "rlm_artifact_write" => (
            "artifact-write",
            &["--session-id", "--name", "--content", "--chunk-id"],
            "artifact name, bytes, storage path",
            json!({ "session_id": "<session_id>", "name": "summary.txt", "content": "findings" }),
            "rlm-mcp artifact-write --session-id <id> --name summary.txt --content findings --json",
        ),
        "rlm_artifact_read" => (
            "artifact-read",
            &["--session-id", "--name", "--start", "--end"],
            "artifact content (optional line slice)",
            json!({ "session_id": "<session_id>", "name": "summary.txt" }),
            "rlm-mcp artifact-read --session-id <id> --name summary.txt --json",
        ),
        "rlm_chunk" => (
            "chunk",
            &[
                "--session-id",
                "--file-pattern",
                "--chunk-id",
                "--offset",
                "--limit",
                "--metadata",
            ],
            "paginated chunks with content and metadata",
            json!({ "session_id": "<session_id>", "chunk_ids": ["c-0"], "limit": 1 }),
            "rlm-mcp chunk --session-id <id> --chunk-id c-0 --json",
        ),
        "rlm_map_plan" => (
            "map-plan",
            &["--session-id", "--chunk-id", "--file-pattern", "--batch-size"],
            "parallel work batches with stable batch_ids and plan_id",
            json!({ "session_id": "<session_id>", "batch_size": 3 }),
            "rlm-mcp map-plan --session-id <id> --batch-size 3 --json",
        ),
        "rlm_map_claim" => (
            "map-claim",
            &["--plan-id", "--worker-id", "--batch-id"],
            "claimed batch_id, chunk_ids, remaining_pending",
            json!({ "plan_id": "<plan_id>", "worker_id": "worker-a" }),
            "rlm-mcp map-claim --plan-id <id> --worker-id worker-a --json",
        ),
        "rlm_map_complete" => (
            "map-complete",
            &["--plan-id", "--worker-id", "--batch-id", "--output"],
            "completion status and all_complete flag",
            json!({
                "plan_id": "<plan_id>",
                "worker_id": "worker-a",
                "batch_id": "batch-0",
                "output": { "batch_id": "batch-0", "findings": [], "unresolved": [] }
            }),
            "rlm-mcp map-complete --plan-id <id> --worker-id worker-a --batch-id batch-0 --output '{\"batch_id\":\"batch-0\",\"findings\":[]}' --json",
        ),
        "rlm_reduce_schema" => (
            "reduce-schema",
            &[],
            "worker_schema, final_answer_schema, checklist",
            json!({}),
            "rlm-mcp reduce-schema --json",
        ),
        "rlm_reduce_merge" => (
            "reduce-merge",
            &["--workers"],
            "merged findings, coverage, needs_recursion flag",
            json!({ "worker_outputs": [{ "batch_id": "b0", "findings": [], "unresolved": [] }] }),
            "rlm-mcp reduce-merge --workers '[{\"batch_id\":\"b0\",\"findings\":[]}]' --json",
        ),
        "rlm_session_list" => (
            "session-list",
            &[],
            "array of active sessions with ids and byte counts",
            json!({}),
            "rlm-mcp session-list --json",
        ),
        "rlm_session_delete" => (
            "session-delete",
            &["--session-id"],
            "deletion confirmation",
            json!({ "session_id": "<session_id>" }),
            "rlm-mcp session-delete --session-id <id> --json",
        ),
        "rlm_session_cleanup" => (
            "session-cleanup",
            &[],
            "removed_count and removed_ids for expired sessions",
            json!({}),
            "rlm-mcp session-cleanup --json",
        ),
        "rlm_session_export" => (
            "session-export",
            &["--session-id"],
            "full session object for backup/transfer",
            json!({ "session_id": "<session_id>" }),
            "rlm-mcp session-export --session-id <id> --json",
        ),
        "rlm_session_import" => (
            "session-import",
            &["--session-json", "--preserve-id", "--stdin"],
            "imported session_id and chunk metadata",
            json!({ "session": { "root_path": "text://x.txt", "chunks": [] }, "preserve_id": false }),
            "rlm-mcp session-import --session-json '{...}' --json",
        ),
        "rlm_task_create" => (
            "task-create",
            &[
                "--session-id",
                "--prompt",
                "--chunk-id",
                "--parent-task-id",
                "--provider",
                "--no-execute",
            ],
            "task_id, root_id, depth, status, token estimates",
            json!({
                "session_id": "<session_id>",
                "prompt": "summarize errors",
                "chunk_ids": ["c-0"],
                "provider": "mock"
            }),
            "rlm-mcp task-create --session-id <id> --prompt \"summarize\" --chunk-id c-0 --json",
        ),
        "rlm_task_list" => (
            "task-list",
            &["--session-id", "--root-id"],
            "tasks in tree with status and depth",
            json!({ "session_id": "<session_id>" }),
            "rlm-mcp task-list --session-id <id> --json",
        ),
        "rlm_task_result" => (
            "task-result",
            &["--task-id"],
            "full task record including structured provider result",
            json!({ "task_id": "<task_id>" }),
            "rlm-mcp task-result --task-id <id> --json",
        ),
        "rlm_task_reduce" => (
            "task-reduce",
            &["--root-id"],
            "merged sub-task findings and aggregate token estimates",
            json!({ "root_id": "<root_id>" }),
            "rlm-mcp task-reduce --root-id <id> --json",
        ),
        "rlm_task_cancel" => (
            "task-cancel",
            &["--root-id", "--reason"],
            "cancelled flag and affected task count",
            json!({ "root_id": "<root_id>", "reason": "budget exceeded" }),
            "rlm-mcp task-cancel --root-id <id> --reason \"budget exceeded\" --json",
        ),
        "rlm_budget_configure" => (
            "budget-configure",
            &[
                "--session-id",
                "--soft-warning",
                "--max-chunks",
                "--max-sub-calls",
                "--max-tokens",
                "--max-wall-secs",
            ],
            "configured limits echo",
            json!({ "session_id": "<session_id>", "mode": "fail_fast", "max_chunks_read": 100 }),
            "rlm-mcp budget-configure --session-id <id> --max-chunks 100 --json",
        ),
        "rlm_budget_status" => (
            "budget-status",
            &["--session-id"],
            "usage, limits, evaluation, tail_cost_percentiles",
            json!({ "session_id": "<session_id>" }),
            "rlm-mcp budget-status --session-id <id> --json",
        ),
        "rlm_trajectory_get" => (
            "trajectory-get",
            &["--session-id", "--format", "--no-redact", "--redact-pattern"],
            "events, summary, optional jsonl or replay steps",
            json!({ "session_id": "<session_id>", "format": "json" }),
            "rlm-mcp trajectory-get --session-id <id> --format json --json",
        ),
        "rlm_trajectory_final" => (
            "trajectory-final",
            &["--session-id", "--answer", "--evidence-count"],
            "recorded final_answer event",
            json!({ "session_id": "<session_id>", "answer": "two ERROR lines", "evidence_count": 2 }),
            "rlm-mcp trajectory-final --session-id <id> --answer \"done\" --json",
        ),
        "rlm_benchmark_list" => (
            "benchmark list",
            &[],
            "available benchmark suites and baselines",
            json!({}),
            "rlm-mcp benchmark list --json",
        ),
        "rlm_benchmark_run" => (
            "benchmark sniah",
            &["--size"],
            "accuracy, per-baseline metrics, qualitative claims",
            json!({ "suite": "sniah", "fixture_size": "mini" }),
            "rlm-mcp benchmark sniah --size mini --json",
        ),
        "rlm_tools_reference" => (
            "tools-reference",
            &[],
            "this structured reference for all tools",
            json!({}),
            "rlm-mcp tools-reference --json",
        ),
        _ => (
            "unknown",
            &[],
            "see input_schema",
            json!({}),
            "rlm-mcp --help",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_covers_every_defined_tool() {
        let defs = crate::mcp::server::McpServer::rmcp_tool_definitions();
        let reference = tools_reference();
        assert_eq!(
            reference["tool_count"].as_u64().unwrap() as usize,
            defs.len()
        );
        for def in defs {
            let name = def.name.as_ref();
            let found = reference["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"].as_str() == Some(name));
            assert!(found, "missing reference for {name}");
        }
    }
}
