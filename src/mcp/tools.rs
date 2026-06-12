use crate::error::{Error, Result};
use crate::rlm::RlmEngine;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ToolHandler {
    rlm: Arc<RlmEngine>,
}

impl ToolHandler {
    pub fn new() -> Self {
        Self {
            rlm: Arc::new(RlmEngine::new()),
        }
    }

    pub fn handle(&self, name: &str, args: &Value) -> Result<Value> {
        match name {
            "rlm_workflow" => {
                let phase = args
                    .get("phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("overview");
                Ok(self.rlm.workflow(phase))
            }
            "rlm_scan" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                self.rlm.scan(path)
            }
            "rlm_chunk" => self.rlm_chunk(args),
            "rlm_peek" => self.rlm_peek(args),
            "rlm_session_list" => Ok(self.rlm.session_list()),
            "rlm_session_delete" => {
                let session_id = require_str(args, "session_id")?;
                self.rlm.session_delete(session_id)
            }
            _ => Err(Error::InvalidArgument(format!("unknown tool: {name}"))),
        }
    }

    fn rlm_chunk(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        self.rlm.chunk(session_id, file_pattern, offset, limit)
    }

    fn rlm_peek(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let query = require_str(args, "query")?;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        self.rlm.peek(session_id, query, limit)
    }
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidArgument(format!("missing {key}")))
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "rlm_workflow",
            "Return RLM loop guidance: overview, load, filter, map, or reduce.",
            json!({
                "type": "object",
                "properties": { "phase": { "type": "string", "default": "overview" } }
            }),
        ),
        tool_def(
            "rlm_scan",
            "Load path into an external RLM session (files, logs, docs). Returns session_id and metadata.",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string", "default": "." } }
            }),
        ),
        tool_def(
            "rlm_peek",
            "Filter/search within a session by substring or path (no full load into context).",
            json!({
                "type": "object",
                "required": ["session_id", "query"],
                "properties": {
                    "session_id": { "type": "string" },
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 20 }
                }
            }),
        ),
        tool_def(
            "rlm_chunk",
            "Read paginated chunks from a session (map phase).",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string" },
                    "file_pattern": { "type": "string" },
                    "offset": { "type": "integer", "default": 0 },
                    "limit": { "type": "integer", "default": 5 }
                }
            }),
        ),
        tool_def(
            "rlm_session_list",
            "List active RLM scan sessions.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_def(
            "rlm_session_delete",
            "Delete an RLM session and free persisted storage.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            }),
        ),
    ]
}

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema
    })
}