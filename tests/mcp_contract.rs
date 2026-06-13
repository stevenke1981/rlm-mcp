use rlm_mcp::mcp::server::McpServer;
use rlm_mcp::mcp::tools::normalized_tools_snapshot;
use rlm_mcp::{test_lock, McpServer as PublicMcpServer};
use rmcp::model::CallToolRequestParams;
use rmcp::{ServerHandler, ServiceExt};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/mcp/tools-list.snapshot.json")
}

fn arguments(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("tool arguments object")
}

fn text_json(result: &rmcp::model::CallToolResult) -> Value {
    let text = result.content[0]
        .raw
        .as_text()
        .expect("text tool result")
        .text
        .as_str();
    serde_json::from_str(text).expect("JSON tool result")
}

#[test]
fn tools_list_matches_snapshot() {
    let snapshot: Value =
        serde_json::from_str(&fs::read_to_string(snapshot_path()).expect("read snapshot"))
            .expect("parse snapshot");
    let current = normalized_tools_snapshot();
    assert_eq!(
        current, snapshot,
        "tools/list drifted from packaging snapshot"
    );
}

#[test]
#[ignore = "run manually: cargo test write_tools_snapshot -- --ignored"]
fn write_tools_snapshot() {
    let content = serde_json::to_string_pretty(&normalized_tools_snapshot()).unwrap();
    fs::write(snapshot_path(), format!("{content}\n")).unwrap();
}

#[test]
fn mcp_initialize_returns_server_info() {
    let info = ServerHandler::get_info(&McpServer::new());
    assert_eq!(info.server_info.name, "rlm-mcp");
    assert!(info.capabilities.tools.is_some());
    assert!(info.capabilities.resources.is_none());
}

#[tokio::test]
async fn official_rmcp_client_preserves_tool_contract_and_rlm_loop() {
    let guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    let server = PublicMcpServer::new();
    drop(guard);

    let (server_transport, client_transport) = tokio::io::duplex(1024 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("start rmcp server")
            .waiting()
            .await
            .expect("wait rmcp server");
    });
    let client = ().serve(client_transport).await.expect("start rmcp client");

    let tools = client.list_all_tools().await.expect("list tools");
    assert_eq!(
        tools.len(),
        normalized_tools_snapshot()["tool_count"].as_u64().unwrap() as usize
    );
    assert!(tools.iter().any(|tool| tool.name == "rlm_workflow"));
    assert!(!tools.iter().any(|tool| tool.name == "index_repository"));

    let reference = client
        .call_tool(CallToolRequestParams::new("rlm_tools_reference"))
        .await
        .expect("tools reference");
    assert_eq!(
        text_json(&reference)["tool_count"].as_u64().unwrap(),
        tools.len() as u64
    );

    let scan = client
        .call_tool(
            CallToolRequestParams::new("rlm_scan").with_arguments(arguments(json!({
                "content": "alpha line\nNEEDLE=42\nomega line\n",
                "virtual_path": "contract/smoke.txt"
            }))),
        )
        .await
        .expect("scan");
    assert_eq!(scan.is_error, Some(false));
    let session_id = text_json(&scan)["session_id"].as_str().unwrap().to_string();

    let peek = client
        .call_tool(
            CallToolRequestParams::new("rlm_peek").with_arguments(arguments(json!({
                "session_id": session_id,
                "query": "NEEDLE"
            }))),
        )
        .await
        .expect("peek");
    assert!(text_json(&peek)["total_match_lines"].as_u64().unwrap() >= 1);

    let chunk = client
        .call_tool(
            CallToolRequestParams::new("rlm_chunk").with_arguments(arguments(json!({
                "session_id": session_id,
                "offset": 0,
                "limit": 1
            }))),
        )
        .await
        .expect("chunk");
    assert_eq!(text_json(&chunk)["chunks"].as_array().unwrap().len(), 1);

    let missing_session = client
        .call_tool(
            CallToolRequestParams::new("rlm_peek").with_arguments(arguments(json!({
                "session_id": "missing-session",
                "query": "NEEDLE"
            }))),
        )
        .await
        .expect("domain errors stay inside tool results");
    assert_eq!(missing_session.is_error, Some(true));

    client.cancel().await.expect("cancel client");
    server_task.await.expect("join server task");
}
