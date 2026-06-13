use rlm_mcp::McpServer;
use rmcp::model::{
    CallToolRequest, CallToolRequestParams, CancelledNotification, CancelledNotificationParam,
    ClientNotification, ClientRequest,
};
use rmcp::service::{PeerRequestOptions, ServiceError};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::json;
use std::time::{Duration, Instant};

fn assert_server_handler<T: ServerHandler>() {}

#[test]
fn rlm_uses_official_rmcp_server_handler() {
    assert_server_handler::<McpServer>();
}

#[tokio::test]
async fn official_client_lists_all_rlm_tools() {
    let (server_transport, client_transport) = tokio::io::duplex(1024 * 1024);
    let server_task = tokio::spawn(async move {
        McpServer::new()
            .serve(server_transport)
            .await
            .expect("start rmcp server")
            .waiting()
            .await
            .expect("wait rmcp server");
    });

    let client = ().serve(client_transport).await.expect("start rmcp client");
    let tools = client.list_all_tools().await.expect("list tools");
    assert_eq!(tools.len(), 33);
    assert!(tools.iter().all(|tool| tool.name.starts_with("rlm_")));

    client.cancel().await.expect("cancel client");
    server_task.await.expect("join server task");
}

fn arguments(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().cloned().expect("tool arguments object")
}

#[tokio::test]
async fn typed_router_rejects_invalid_provider_as_invalid_params() {
    let (server_transport, client_transport) = tokio::io::duplex(1024 * 1024);
    let server_task = tokio::spawn(async move {
        McpServer::new()
            .serve(server_transport)
            .await
            .expect("start rmcp server")
            .waiting()
            .await
            .expect("wait rmcp server");
    });

    let client = ().serve(client_transport).await.expect("start rmcp client");
    let err = client
        .call_tool(
            CallToolRequestParams::new("rlm_task_create").with_arguments(arguments(json!({
                "session_id": "dummy-session",
                "prompt": "try invalid provider",
                "provider": "bogus"
            }))),
        )
        .await
        .expect_err("typed Schemars router should reject invalid enum before domain handler");
    assert!(err.to_string().contains("failed to deserialize parameters"));

    client.cancel().await.expect("cancel client");
    server_task.await.expect("join server task");
}

#[tokio::test]
async fn mcp_request_cancellation_bounds_slow_tool_call() {
    let (server_transport, client_transport) = tokio::io::duplex(1024 * 1024);
    let server_task = tokio::spawn(async move {
        McpServer::new()
            .serve(server_transport)
            .await
            .expect("start rmcp server")
            .waiting()
            .await
            .expect("wait rmcp server");
    });

    let client = ().serve(client_transport).await.expect("start rmcp client");
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(
        CallToolRequestParams::new("rlm_benchmark_run").with_arguments(arguments(json!({
            "suite": "sniah",
            "fixture_size": "nightly"
        }))),
    ));
    let handle = client
        .peer()
        .send_cancellable_request(request, PeerRequestOptions::no_options())
        .await
        .expect("send cancellable benchmark request");
    let request_id = handle.id.clone();
    let started = Instant::now();
    client
        .peer()
        .send_notification(ClientNotification::CancelledNotification(
            CancelledNotification::new(CancelledNotificationParam {
                request_id,
                reason: Some("test cancellation".into()),
            }),
        ))
        .await
        .expect("send cancellation notification");

    let err = tokio::time::timeout(Duration::from_secs(2), handle.await_response())
        .await
        .expect("cancelled request should respond within bound")
        .expect_err("cancelled request should not complete successfully");
    assert!(matches!(err, ServiceError::Cancelled { .. }));
    assert!(started.elapsed() < Duration::from_secs(2));

    client.cancel().await.expect("cancel client");
    server_task.await.expect("join server task");
}
