use rlm_mcp::McpServer;
use rmcp::{ServerHandler, ServiceExt};

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
