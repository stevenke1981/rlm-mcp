use crate::error::{Error, Result as RlmResult};
use crate::mcp::tools::{tool_definitions, ToolHandler};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{MaybeSendFuture, RequestContext};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;

pub const SERVER_NAME: &str = "rlm-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct McpServer {
    handler: ToolHandler,
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            handler: ToolHandler::new(),
        }
    }

    pub async fn serve_stdio(self) -> RlmResult<()> {
        self.serve(rmcp::transport::stdio())
            .await
            .map_err(|error| Error::Other(format!("failed to start MCP stdio service: {error}")))?
            .waiting()
            .await
            .map(|_| ())
            .map_err(|error| Error::Other(format!("MCP stdio service failed: {error}")))
    }

    pub fn rmcp_tool_definitions() -> Vec<Tool> {
        tool_definitions()
            .into_iter()
            .map(|definition| {
                serde_json::from_value(definition).expect("RLM MCP tool definition must be valid")
            })
            .collect()
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
            .with_instructions(
                "Standalone RLM MCP server. Load external context with rlm_scan, then filter with rlm_peek, map with rlm_chunk/map tools, reduce, and recurse when evidence is incomplete. Independent of any graph index.",
            )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<ListToolsResult, ErrorData>>
           + MaybeSendFuture
           + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: Self::rmcp_tool_definitions(),
            ..Default::default()
        }))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        Self::rmcp_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == name)
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<CallToolResult, ErrorData>>
           + MaybeSendFuture
           + '_ {
        let handler = self.handler.clone();
        async move {
            let name = request.name.into_owned();
            let arguments = Value::Object(request.arguments.unwrap_or_default());
            let result =
                tokio::task::spawn_blocking(move || handler.handle(&name, &arguments)).await;
            Ok(match result {
                Ok(Ok(value)) => match serde_json::to_string_pretty(&value) {
                    Ok(text) => CallToolResult::success(vec![Content::text(text)]),
                    Err(error) => tool_error(format!("failed to encode tool result: {error}")),
                },
                Ok(Err(error)) => tool_error(error.to_string()),
                Err(error) => {
                    tracing::error!(%error, "RLM tool worker failed");
                    tool_error("internal tool worker failure")
                }
            })
        }
    }
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(message.into())])
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}
