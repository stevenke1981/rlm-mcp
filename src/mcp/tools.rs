use crate::cbm_client::CbmClient;
use crate::config::default_project;
use crate::error::{Error, Result};
use crate::project::normalize_project_name;
use crate::rlm::RlmEngine;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ToolHandler {
    rlm: Arc<RlmEngine>,
}

impl ToolHandler {
    pub fn new() -> Self {
        let cbm = Arc::new(CbmClient::new());
        Self {
            rlm: Arc::new(RlmEngine::new(cbm)),
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
            "rlm_index_status" => {
                let project = resolve_project(args)?;
                self.rlm.index_status(&project)
            }
            "rlm_filter" => self.rlm_filter(args),
            "rlm_read_symbol" => self.rlm_read_symbol(args),
            "rlm_trace" => self.rlm_trace(args),
            "rlm_architecture" => {
                let project = resolve_project(args)?;
                self.rlm.architecture(&project)
            }
            "rlm_detect_changes" => {
                let project = resolve_project(args)?;
                let scope = args.get("scope").and_then(|v| v.as_str());
                self.rlm.detect_changes(&project, scope)
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

    fn rlm_filter(&self, args: &Value) -> Result<Value> {
        let project = resolve_project(args)?;
        let query = args.get("query").and_then(|v| v.as_str());
        let pattern = args
            .get("pattern")
            .or_else(|| args.get("name_pattern"))
            .and_then(|v| v.as_str());
        let label = args.get("label").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);
        self.rlm.filter(&project, query, pattern, label, limit)
    }

    fn rlm_read_symbol(&self, args: &Value) -> Result<Value> {
        let project = resolve_project(args)?;
        let qn = require_str(args, "qualified_name")?;
        self.rlm.read_symbol(&project, qn)
    }

    fn rlm_trace(&self, args: &Value) -> Result<Value> {
        let project = resolve_project(args)?;
        let function_name = require_str(args, "function_name")?;
        let direction = args
            .get("direction")
            .and_then(|v| v.as_str())
            .unwrap_or("both");
        let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3);
        let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("calls");
        self.rlm.trace(&project, function_name, direction, depth, mode)
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

fn resolve_project(args: &Value) -> Result<String> {
    if let Some(p) = args.get("project").and_then(|v| v.as_str()) {
        return Ok(normalize_project_name(p));
    }
    default_project()
        .map(|p| normalize_project_name(&p))
        .ok_or_else(|| {
            Error::InvalidArgument("project is required (or set CBM_PROJECT env var)".into())
        })
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "rlm_workflow",
            "Return RLM loop guidance for overview, filter, map, or reduce.",
            json!({
                "type": "object",
                "properties": { "phase": { "type": "string", "default": "overview" } }
            }),
        ),
        tool_def(
            "rlm_index_status",
            "Check codebase-memory-mcp index status for a project.",
            project_schema(false),
        ),
        tool_def(
            "rlm_filter",
            "Filter candidates via search_graph (query/label) or search_code files mode (pattern).",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string" },
                    "query": { "type": "string" },
                    "pattern": { "type": "string" },
                    "label": { "type": "string" },
                    "limit": { "type": "integer", "default": 20 }
                }
            }),
        ),
        tool_def(
            "rlm_read_symbol",
            "Read one symbol for Map phase (get_code_snippet wrapper).",
            json!({
                "type": "object",
                "required": ["qualified_name"],
                "properties": {
                    "project": { "type": "string" },
                    "qualified_name": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "rlm_trace",
            "Trace call/data-flow paths via trace_path.",
            json!({
                "type": "object",
                "required": ["function_name"],
                "properties": {
                    "project": { "type": "string" },
                    "function_name": { "type": "string" },
                    "direction": { "type": "string", "default": "both" },
                    "depth": { "type": "integer", "default": 3 },
                    "mode": { "type": "string", "default": "calls" }
                }
            }),
        ),
        tool_def(
            "rlm_architecture",
            "High-level architecture overview from codebase-memory-mcp graph.",
            project_schema(false),
        ),
        tool_def(
            "rlm_detect_changes",
            "Detect git changes and impact via codebase-memory-mcp.",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string" },
                    "scope": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "rlm_scan",
            "Scan directory into an RLM session (logs/CSV/non-graph files).",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string", "default": "." } }
            }),
        ),
        tool_def(
            "rlm_peek",
            "Peek query snippets inside an RLM session.",
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
            "Get paginated chunks from an RLM session.",
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
            "Delete an RLM session and free memory.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            }),
        ),
    ]
}

fn project_schema(required: bool) -> Value {
    if required {
        json!({
            "type": "object",
            "required": ["project"],
            "properties": { "project": { "type": "string" } }
        })
    } else {
        json!({
            "type": "object",
            "properties": { "project": { "type": "string" } }
        })
    }
}

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema
    })
}