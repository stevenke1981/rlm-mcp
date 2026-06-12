use crate::error::{Error, Result};
use crate::mcp::tools::{tool_definitions, ToolHandler};
use crate::mcp::transport::{read_stdin_message, write_stdout_message};
use serde_json::{json, Value};

pub const SERVER_NAME: &str = "codebase-memory-rlm-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct McpServer {
    handler: ToolHandler,
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            handler: ToolHandler::new(),
        }
    }

    pub fn run(&self) -> Result<()> {
        while let Some(line) = read_stdin_message()? {
            let response = self.handle_message(&line)?;
            if let Some(body) = response {
                write_stdout_message(&body)?;
            }
        }
        Ok(())
    }

    pub fn handle_message(&self, raw: &str) -> Result<Option<String>> {
        let request: Value = serde_json::from_str(raw)?;
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let result = match method {
            "initialize" => Ok(self.handle_initialize()),
            "notifications/initialized" | "initialized" => return Ok(None),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.handle_tool_call(&request),
            _ => {
                if id.is_none() {
                    return Ok(None);
                }
                Err(Error::InvalidArgument(format!("unknown method: {method}")))
            }
        };

        match (id, result) {
            (None, _) => Ok(None),
            (Some(id), Ok(value)) => Ok(Some(format_response(id, value)?)),
            (Some(id), Err(e)) => Ok(Some(format_error(id, -32603, &e.to_string())?)),
        }
    }

    fn handle_initialize(&self) -> Value {
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            },
            "instructions": "Standalone RLM MCP server. External context via rlm_scan sessions. Loop: load → filter (rlm_peek) → map (rlm_chunk) → reduce. Independent of any graph index."
        })
    }

    fn handle_tool_call(&self, request: &Value) -> Result<Value> {
        let params = request
            .get("params")
            .ok_or_else(|| Error::InvalidArgument("missing params".into()))?;
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidArgument("missing tool name".into()))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let result = self.handler.handle(name, &args)?;
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result)?
            }],
            "isError": false
        }))
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

fn format_response(id: Value, result: Value) -> Result<String> {
    Ok(serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    }))?)
}

fn format_error(id: Value, code: i32, message: &str) -> Result<String> {
    Ok(serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_initialize() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let resp = server.handle_message(&req.to_string()).unwrap().unwrap();
        assert!(resp.contains("codebase-memory-rlm-mcp"));
    }

    #[test]
    fn lists_rlm_tools() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        let resp = server.handle_message(&req.to_string()).unwrap().unwrap();
        assert!(resp.contains("rlm_workflow"));
        assert!(resp.contains("rlm_scan"));
        assert!(resp.contains("rlm_env_info"));
        assert!(resp.contains("rlm_map_plan"));
        assert!(resp.contains("rlm_reduce_merge"));
        assert!(resp.contains("rlm_task_create"));
        assert!(!resp.contains("rlm_filter"));
        assert!(!resp.contains("index_repository"));
    }
}
