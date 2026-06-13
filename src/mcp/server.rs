use crate::error::{Error, Result as RlmResult};
use crate::mcp::params::*;
use crate::mcp::tools::ToolHandler;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{
    CallToolResult, Content, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const SERVER_NAME: &str = "rlm-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct McpServer {
    handler: ToolHandler,
    tool_router: ToolRouter<Self>,
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            handler: ToolHandler::new(),
            tool_router: Self::tool_router(),
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
        normalize_tool_schemas(Self::tool_router().list_all())
    }

    async fn invoke_tool<P>(
        &self,
        name: &'static str,
        params: P,
        cancellation: CancellationToken,
    ) -> std::result::Result<CallToolResult, ErrorData>
    where
        P: Serialize + Send + 'static,
    {
        if cancellation.is_cancelled() {
            return Err(ErrorData::internal_error("request cancelled", None));
        }
        let handler = self.handler.clone();
        let args = serialize_params(params)?;
        let result = tokio::task::spawn_blocking(move || handler.handle(name, &args));
        tokio::select! {
            _ = cancellation.cancelled() => {
                Err(ErrorData::internal_error("request cancelled", None))
            }
            joined = result => {
                Ok(match joined {
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
}

#[tool_router(router = tool_router)]
impl McpServer {
    #[tool(
        name = "rlm_workflow",
        description = "Return RLM loop guidance: overview, load, filter, map, or reduce."
    )]
    async fn rlm_workflow(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<WorkflowInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_workflow", params, cancellation).await
    }

    #[tool(
        name = "rlm_scan",
        description = "Load path or text content into an external RLM session. Returns session_id and metadata."
    )]
    async fn rlm_scan(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<ScanInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_scan", params, cancellation).await
    }

    #[tool(
        name = "rlm_env_info",
        description = "Inspect session as external environment: files, chunk IDs, bytes, expiry."
    )]
    async fn rlm_env_info(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<SessionIdInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_env_info", params, cancellation).await
    }

    #[tool(
        name = "rlm_peek",
        description = "Filter/search within a session (substring, BM25, glob, regex, line range) without full load."
    )]
    async fn rlm_peek(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<PeekInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_peek", params, cancellation).await
    }

    #[tool(
        name = "rlm_slice",
        description = "Read a line range from a specific chunk (REPL-style slice)."
    )]
    async fn rlm_slice(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<SliceInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_slice", params, cancellation).await
    }

    #[tool(
        name = "rlm_repl_info",
        description = "List REPL sandbox backends, capability flags, and execution limits (default is safe non-executable)."
    )]
    async fn rlm_repl_info(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<EmptyInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_repl_info", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_repl_execute",
        description = "Execute code via an opt-in REPL sandbox backend (requires RLM_ALLOW_REPL_EXEC=1)."
    )]
    async fn rlm_repl_execute(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<ReplExecuteInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_repl_execute", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_transform",
        description = "Apply safe deterministic text transforms (no code execution)."
    )]
    async fn rlm_transform(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<TransformInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_transform", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_artifact_write",
        description = "Persist derived text under the session artifact store."
    )]
    async fn rlm_artifact_write(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<ArtifactWriteInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_artifact_write", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_artifact_read",
        description = "Read a session artifact (optional line range)."
    )]
    async fn rlm_artifact_read(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<ArtifactReadInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_artifact_read", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_chunk",
        description = "Read paginated chunks by file pattern or chunk_ids (map phase)."
    )]
    async fn rlm_chunk(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<ChunkInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_chunk", params, cancellation).await
    }

    #[tool(
        name = "rlm_map_plan",
        description = "Create parallel work batches from chunk_ids or file pattern."
    )]
    async fn rlm_map_plan(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<MapPlanInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_map_plan", params, cancellation).await
    }

    #[tool(
        name = "rlm_map_claim",
        description = "Claim the next unclaimed batch (or a specific batch_id) for a worker."
    )]
    async fn rlm_map_claim(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<MapClaimInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_map_claim", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_map_complete",
        description = "Mark a claimed batch complete and store worker JSON output."
    )]
    async fn rlm_map_complete(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<MapCompleteInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_map_complete", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_reduce_schema",
        description = "Return JSON schema and checklist for the reduce phase."
    )]
    async fn rlm_reduce_schema(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<EmptyInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_reduce_schema", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_reduce_merge",
        description = "Merge worker JSON outputs from map phase into reducible findings."
    )]
    async fn rlm_reduce_merge(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<ReduceMergeInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_reduce_merge", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_session_list",
        description = "List active RLM scan sessions."
    )]
    async fn rlm_session_list(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<EmptyInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_session_list", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_session_delete",
        description = "Delete an RLM session and free persisted storage."
    )]
    async fn rlm_session_delete(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<SessionIdInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_session_delete", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_session_cleanup",
        description = "Remove expired sessions from cache (safe for concurrent readers)."
    )]
    async fn rlm_session_cleanup(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<EmptyInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_session_cleanup", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_session_export",
        description = "Export a full session JSON blob for backup or transfer."
    )]
    async fn rlm_session_export(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<SessionIdInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_session_export", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_session_import",
        description = "Import a session JSON blob (optionally preserve session_id)."
    )]
    async fn rlm_session_import(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<SessionImportInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_session_import", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_task_create",
        description = "Create a recursive sub-task over session chunks (mock/dry-run/command/openai providers)."
    )]
    async fn rlm_task_create(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<TaskCreateInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_task_create", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_task_list",
        description = "List recursive sub-tasks, optionally filtered by session_id or root_id."
    )]
    async fn rlm_task_list(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<TaskListInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_task_list", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_task_result",
        description = "Get full result for a sub-task by task_id."
    )]
    async fn rlm_task_result(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<TaskIdInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_task_result", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_task_reduce",
        description = "Reduce all sub-tasks under a root_id into merged findings and cost estimates."
    )]
    async fn rlm_task_reduce(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<RootIdInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_task_reduce", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_task_cancel",
        description = "Cancel a task tree and mark pending tasks as cancelled."
    )]
    async fn rlm_task_cancel(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<TaskCancelInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_task_cancel", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_budget_configure",
        description = "Configure per-session budget limits and fail-fast/soft-warning mode."
    )]
    async fn rlm_budget_configure(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<BudgetConfigureInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_budget_configure", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_budget_status",
        description = "Report session budget usage, limits, and tail-cost variance."
    )]
    async fn rlm_budget_status(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<SessionIdInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_budget_status", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_trajectory_get",
        description = "Get persisted RLM run trajectory with cost summary (json, jsonl, or replay format)."
    )]
    async fn rlm_trajectory_get(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<TrajectoryGetInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_trajectory_get", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_trajectory_final",
        description = "Record final answer event in trajectory log."
    )]
    async fn rlm_trajectory_final(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<TrajectoryFinalInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_trajectory_final", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_benchmark_list",
        description = "List available RLM benchmark suites (S-NIAH mini-suite for CI)."
    )]
    async fn rlm_benchmark_list(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<EmptyInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_benchmark_list", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_benchmark_run",
        description = "Run an offline benchmark suite and record accuracy, cost, runtime, and trajectory metrics."
    )]
    async fn rlm_benchmark_run(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<BenchmarkRunInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_benchmark_run", params, cancellation)
            .await
    }

    #[tool(
        name = "rlm_tools_reference",
        description = "Return structured schema reference for every MCP tool (CLI mapping, returns, examples)."
    )]
    async fn rlm_tools_reference(
        &self,
        cancellation: CancellationToken,
        Parameters(params): Parameters<EmptyInput>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        self.invoke_tool("rlm_tools_reference", params, cancellation)
            .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: normalize_tool_schemas(self.tool_router.list_all()),
            meta: None,
            next_cursor: None,
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router
            .get(name)
            .cloned()
            .map(normalize_tool_schema)
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
            .with_instructions(
                "Standalone RLM MCP server. Load external context with rlm_scan, then filter with rlm_peek, map with rlm_chunk/map tools, reduce, and recurse when evidence is incomplete. Independent of any graph index.",
            )
    }
}

fn serialize_params<P: Serialize>(params: P) -> std::result::Result<Value, ErrorData> {
    serde_json::to_value(params).map_err(|error| {
        ErrorData::internal_error(format!("failed to encode tool arguments: {error}"), None)
    })
}

fn normalize_tool_schemas(tools: Vec<Tool>) -> Vec<Tool> {
    tools.into_iter().map(normalize_tool_schema).collect()
}

fn normalize_tool_schema(mut tool: Tool) -> Tool {
    let mut schema = Value::Object(tool.input_schema.as_ref().clone());
    normalize_json_schema_node(&mut schema);
    if let Value::Object(object) = schema {
        tool.input_schema = Arc::new(object);
    }
    tool
}

fn normalize_json_schema_node(value: &mut Value) {
    match value {
        Value::Bool(_) => {
            *value = Value::Object(Default::default());
        }
        Value::Object(object) => {
            for key in ["properties", "patternProperties", "$defs", "definitions"] {
                if let Some(Value::Object(children)) = object.get_mut(key) {
                    for child in children.values_mut() {
                        normalize_json_schema_node(child);
                    }
                }
            }
            for key in [
                "items",
                "additionalProperties",
                "contains",
                "not",
                "if",
                "then",
                "else",
                "propertyNames",
            ] {
                if let Some(child) = object.get_mut(key) {
                    normalize_json_schema_node(child);
                }
            }
            for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
                if let Some(Value::Array(items)) = object.get_mut(key) {
                    for item in items {
                        normalize_json_schema_node(item);
                    }
                }
            }
        }
        _ => {}
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
