use rlm_mcp::mcp::server::McpServer;
use rlm_mcp::mcp::tools::normalized_tools_snapshot;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/mcp/tools-list.snapshot.json")
}

fn parse_response(body: &str) -> Value {
    serde_json::from_str(body).expect("valid json-rpc response")
}

#[test]
fn tools_list_matches_snapshot() {
    let snapshot: Value =
        serde_json::from_str(&fs::read_to_string(snapshot_path()).expect("read snapshot"))
            .expect("parse snapshot");
    let current = normalized_tools_snapshot();
    assert_eq!(current, snapshot, "tools/list drifted from packaging snapshot");
}

#[test]
#[ignore = "run manually: cargo test write_tools_snapshot -- --ignored"]
fn write_tools_snapshot() {
    let content = serde_json::to_string_pretty(&normalized_tools_snapshot()).unwrap();
    fs::write(snapshot_path(), format!("{content}\n")).unwrap();
}

#[test]
fn mcp_initialize_returns_server_info() {
    let server = McpServer::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let body = server
        .handle_message(&req.to_string())
        .unwrap()
        .expect("response");
    let resp = parse_response(&body);
    assert_eq!(resp["result"]["serverInfo"]["name"], "rlm-mcp");
}

#[test]
fn mcp_tools_list_matches_snapshot_tools() {
    let server = McpServer::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let body = server
        .handle_message(&req.to_string())
        .unwrap()
        .expect("response");
    let resp = parse_response(&body);
    let listed = &resp["result"]["tools"];
    assert_eq!(
        listed.as_array().unwrap().len(),
        normalized_tools_snapshot()["tool_count"].as_u64().unwrap() as usize
    );
}

#[test]
fn mcp_tools_reference_covers_all_tools() {
    let server = McpServer::new();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "rlm_tools_reference",
            "arguments": {}
        }
    });
    let body = server
        .handle_message(&req.to_string())
        .unwrap()
        .expect("reference response");
    let resp = parse_response(&body);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let reference: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        reference["tool_count"].as_u64().unwrap(),
        normalized_tools_snapshot()["tool_count"].as_u64().unwrap()
    );
}

#[test]
fn mcp_scan_peek_chunk_smoke() {
    let server = McpServer::new();

    let scan_req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "rlm_scan",
            "arguments": {
                "content": "alpha line\nNEEDLE=42\nomega line\n",
                "virtual_path": "contract/smoke.txt"
            }
        }
    });
    let scan_body = server
        .handle_message(&scan_req.to_string())
        .unwrap()
        .expect("scan response");
    let scan_resp = parse_response(&scan_body);
    let scan_text = scan_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("scan text");
    let scan_json: Value = serde_json::from_str(scan_text).unwrap();
    let session_id = scan_json["session_id"].as_str().unwrap();

    let peek_req = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "rlm_peek",
            "arguments": {
                "session_id": session_id,
                "query": "NEEDLE"
            }
        }
    });
    let peek_body = server
        .handle_message(&peek_req.to_string())
        .unwrap()
        .expect("peek response");
    let peek_resp = parse_response(&peek_body);
    let peek_text = peek_resp["result"]["content"][0]["text"].as_str().unwrap();
    let peek_json: Value = serde_json::from_str(peek_text).unwrap();
    assert!(peek_json["total_match_lines"].as_u64().unwrap() >= 1);

    let chunk_req = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": {
            "name": "rlm_chunk",
            "arguments": {
                "session_id": session_id,
                "offset": 0,
                "limit": 1
            }
        }
    });
    let chunk_body = server
        .handle_message(&chunk_req.to_string())
        .unwrap()
        .expect("chunk response");
    let chunk_resp = parse_response(&chunk_body);
    let chunk_text = chunk_resp["result"]["content"][0]["text"].as_str().unwrap();
    let chunk_json: Value = serde_json::from_str(chunk_text).unwrap();
    assert_eq!(chunk_json["chunks"].as_array().unwrap().len(), 1);
}