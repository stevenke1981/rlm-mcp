use crate::error::{Error, Result};
use crate::rlm::{BudgetMode, PeekOptions, RlmEngine, SessionBudget, TaskBudget};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct ToolHandler {
    rlm: Arc<RlmEngine>,
}

impl Default for ToolHandler {
    fn default() -> Self {
        Self {
            rlm: Arc::new(RlmEngine::new()),
        }
    }
}

impl ToolHandler {
    pub fn new() -> Self {
        Self::default()
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
            "rlm_scan" => self.rlm_scan(args),
            "rlm_env_info" => {
                let session_id = require_str(args, "session_id")?;
                self.rlm.env_info(session_id)
            }
            "rlm_slice" => {
                let session_id = require_str(args, "session_id")?;
                let chunk_id = require_str(args, "chunk_id")?;
                let start = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let end = args
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(start as u64) as usize;
                self.rlm.slice(session_id, chunk_id, start, end)
            }
            "rlm_transform" => self.rlm_transform(args),
            "rlm_repl_info" => Ok(self.rlm.repl_info()),
            "rlm_repl_execute" => {
                let session_id = require_str(args, "session_id")?;
                let code = require_str(args, "code")?;
                let language = args.get("language").and_then(|v| v.as_str());
                let backend = args.get("backend").and_then(|v| v.as_str());
                self.rlm.repl_execute(session_id, code, language, backend)
            }
            "rlm_artifact_write" => self.rlm_artifact_write(args),
            "rlm_artifact_read" => self.rlm_artifact_read(args),
            "rlm_chunk" => self.rlm_chunk(args),
            "rlm_peek" => self.rlm_peek(args),
            "rlm_map_plan" => self.rlm_map_plan(args),
            "rlm_map_claim" => self.rlm_map_claim(args),
            "rlm_map_complete" => self.rlm_map_complete(args),
            "rlm_reduce_schema" => Ok(self.rlm.reduce_schema()),
            "rlm_reduce_merge" => self.rlm_reduce_merge(args),
            "rlm_session_list" => Ok(self.rlm.session_list()),
            "rlm_session_delete" => {
                let session_id = require_str(args, "session_id")?;
                self.rlm.session_delete(session_id)
            }
            "rlm_session_cleanup" => self.rlm.session_cleanup(),
            "rlm_session_export" => {
                let session_id = require_str(args, "session_id")?;
                self.rlm.session_export(session_id)
            }
            "rlm_session_import" => {
                let preserve_id = args
                    .get("preserve_id")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let session_val = args
                    .get("session")
                    .ok_or_else(|| Error::InvalidArgument("missing session".into()))?;
                let session: crate::rlm::ScanSession = serde_json::from_value(session_val.clone())?;
                self.rlm.session_import(session, preserve_id)
            }
            "rlm_task_create" => self.rlm_task_create(args),
            "rlm_task_list" => Ok(self.rlm.task_list(
                args.get("session_id").and_then(|v| v.as_str()),
                args.get("root_id").and_then(|v| v.as_str()),
            )),
            "rlm_task_result" => {
                let task_id = require_str(args, "task_id")?;
                self.rlm.task_result(task_id)
            }
            "rlm_task_reduce" => {
                let root_id = require_str(args, "root_id")?;
                self.rlm.task_reduce(root_id)
            }
            "rlm_task_cancel" => {
                let root_id = require_str(args, "root_id")?;
                let reason = args
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("cancelled by agent");
                self.rlm.task_cancel(root_id, reason)
            }
            "rlm_budget_configure" => {
                let session_id = require_str(args, "session_id")?;
                let config = parse_session_budget(session_id, args)?;
                self.rlm.budget_configure(config)
            }
            "rlm_budget_status" => {
                let session_id = require_str(args, "session_id")?;
                Ok(self.rlm.budget_status(session_id))
            }
            "rlm_trajectory_get" => {
                let session_id = require_str(args, "session_id")?;
                let format = args
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or("json");
                let redact = args.get("redact").and_then(|v| v.as_bool()).unwrap_or(true);
                let patterns = args
                    .get("redact_patterns")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                self.rlm
                    .trajectory_get(session_id, format, redact, &patterns)
            }
            "rlm_trajectory_final" => {
                let session_id = require_str(args, "session_id")?;
                let answer = require_str(args, "answer")?;
                let evidence_count = args
                    .get("evidence_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                Ok(self
                    .rlm
                    .trajectory_record_final(session_id, answer, evidence_count))
            }
            "rlm_benchmark_list" => Ok(crate::benchmark::list_suites()),
            "rlm_benchmark_run" => {
                let suite = args
                    .get("suite")
                    .and_then(|v| v.as_str())
                    .unwrap_or("sniah");
                let fixture_size = args.get("fixture_size").and_then(|v| v.as_str());
                crate::benchmark::run_suite(&self.rlm, suite, fixture_size)
            }
            "rlm_tools_reference" => Ok(crate::mcp::schema_docs::tools_reference()),
            _ => Err(Error::InvalidArgument(format!("unknown tool: {name}"))),
        }
    }

    fn rlm_scan(&self, args: &Value) -> Result<Value> {
        let path = args.get("path").and_then(|v| v.as_str());
        let content = args.get("content").and_then(|v| v.as_str());
        let virtual_path = args.get("virtual_path").and_then(|v| v.as_str());
        let variable = args.get("variable_name").and_then(|v| v.as_str());
        self.rlm.scan(path, content, virtual_path, variable)
    }

    fn rlm_transform(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let operation = require_str(args, "operation")?;
        let params = args.get("params").cloned().unwrap_or(json!({}));
        let chunk_id = args.get("chunk_id").and_then(|v| v.as_str());
        let artifact_name = args.get("artifact_name").and_then(|v| v.as_str());
        let content = args.get("content").and_then(|v| v.as_str());
        self.rlm.transform(
            session_id,
            operation,
            &params,
            chunk_id,
            artifact_name,
            content,
        )
    }

    fn rlm_artifact_write(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let name = require_str(args, "name")?;
        let content = args.get("content").and_then(|v| v.as_str());
        let source_chunk_id = args.get("source_chunk_id").and_then(|v| v.as_str());
        self.rlm
            .artifact_write(session_id, name, content, source_chunk_id)
    }

    fn rlm_artifact_read(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let name = require_str(args, "name")?;
        let start_line = args
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let end_line = args
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        self.rlm
            .artifact_read(session_id, name, start_line, end_line)
    }

    fn rlm_chunk(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());
        let chunk_ids = args.get("chunk_ids").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        });
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let include_metadata = args
            .get("include_metadata")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        self.rlm.chunk(
            session_id,
            file_pattern,
            chunk_ids.as_deref(),
            offset,
            limit,
            include_metadata,
        )
    }

    fn rlm_peek(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let opts = PeekOptions {
            query: args.get("query").and_then(|v| v.as_str()),
            path_filter: args.get("path_filter").and_then(|v| v.as_str()),
            glob: args.get("glob").and_then(|v| v.as_str()),
            regex: args.get("regex").and_then(|v| v.as_bool()).unwrap_or(false),
            bm25: args.get("bm25").and_then(|v| v.as_bool()).unwrap_or(false),
            case_sensitive: args
                .get("case_sensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            line_start: args
                .get("line_start")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize),
            line_end: args
                .get("line_end")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize),
            context_radius: args
                .get("context_radius")
                .and_then(|v| v.as_u64())
                .unwrap_or(2) as usize,
            limit: args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize,
            include_content: args
                .get("include_content")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };
        if opts.bm25 && opts.query.is_none() {
            return Err(Error::InvalidArgument("bm25 search requires query".into()));
        }
        if opts.bm25 && opts.regex {
            return Err(Error::InvalidArgument(
                "bm25 and regex are mutually exclusive".into(),
            ));
        }
        if opts.query.is_none() && opts.path_filter.is_none() && opts.glob.is_none() {
            return Err(Error::InvalidArgument(
                "provide query, path_filter, or glob".into(),
            ));
        }
        self.rlm.peek(session_id, opts)
    }

    fn rlm_map_plan(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let chunk_ids = args.get("chunk_ids").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        });
        let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());
        let batch_size = args.get("batch_size").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        self.rlm
            .map_plan(session_id, chunk_ids.as_deref(), file_pattern, batch_size)
    }

    fn rlm_map_claim(&self, args: &Value) -> Result<Value> {
        let plan_id = require_str(args, "plan_id")?;
        let worker_id = require_str(args, "worker_id")?;
        let batch_id = args.get("batch_id").and_then(|v| v.as_str());
        self.rlm.map_claim(plan_id, worker_id, batch_id)
    }

    fn rlm_map_complete(&self, args: &Value) -> Result<Value> {
        let plan_id = require_str(args, "plan_id")?;
        let worker_id = require_str(args, "worker_id")?;
        let batch_id = require_str(args, "batch_id")?;
        let output = args
            .get("output")
            .cloned()
            .ok_or_else(|| Error::InvalidArgument("missing output".into()))?;
        self.rlm.map_complete(plan_id, worker_id, batch_id, output)
    }

    fn rlm_reduce_merge(&self, args: &Value) -> Result<Value> {
        let workers = args
            .get("worker_outputs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        self.rlm.reduce_merge(&workers)
    }

    fn rlm_task_create(&self, args: &Value) -> Result<Value> {
        let session_id = require_str(args, "session_id")?;
        let prompt = require_str(args, "prompt")?;
        let chunk_ids = args
            .get("chunk_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let parent_task_id = args.get("parent_task_id").and_then(|v| v.as_str());
        let provider = args
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("mock");
        let execute = args
            .get("execute")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let budget = parse_budget(args);
        let budget_mode = parse_budget_mode(args);
        self.rlm.task_create(
            session_id,
            prompt,
            &chunk_ids,
            parent_task_id,
            provider,
            budget,
            budget_mode,
            execute,
        )
    }
}

fn parse_budget_mode(args: &Value) -> Option<BudgetMode> {
    args.get("budget_mode")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "soft_warning" | "soft-warning" => Some(BudgetMode::SoftWarning),
            "fail_fast" | "fail-fast" => Some(BudgetMode::FailFast),
            _ => None,
        })
}

fn parse_session_budget(session_id: &str, args: &Value) -> Result<SessionBudget> {
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "soft_warning" | "soft-warning" => BudgetMode::SoftWarning,
            _ => BudgetMode::FailFast,
        })
        .unwrap_or_default();
    let task_budget = args
        .get("task_budget")
        .and_then(parse_budget)
        .unwrap_or_default();
    Ok(SessionBudget {
        session_id: session_id.into(),
        mode,
        max_chunks_read: args
            .get("max_chunks_read")
            .and_then(|v| v.as_u64())
            .unwrap_or(500),
        max_sub_calls: args
            .get("max_sub_calls")
            .and_then(|v| v.as_u64())
            .unwrap_or(64),
        max_total_tokens_est: args
            .get("max_total_tokens_est")
            .and_then(|v| v.as_u64())
            .unwrap_or(500_000),
        max_wall_secs: args
            .get("max_wall_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(600),
        task_budget,
    })
}

fn parse_budget(args: &Value) -> Option<TaskBudget> {
    let b = args.get("budget")?;
    Some(TaskBudget {
        max_depth: b.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(4) as u32,
        max_fanout: b.get("max_fanout").and_then(|v| v.as_u64()).unwrap_or(8) as u32,
        max_subcalls: b.get("max_subcalls").and_then(|v| v.as_u64()).unwrap_or(32) as u32,
        max_input_bytes: b
            .get("max_input_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(256 * 1024) as usize,
        max_wall_secs: b
            .get("max_wall_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(300),
    })
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidArgument(format!("missing {key}")))
}

/// Canonical tools/list payload for contract tests and packaging snapshot.
pub fn normalized_tools_snapshot() -> Value {
    let tools: Vec<Value> = crate::mcp::server::McpServer::rmcp_tool_definitions()
        .into_iter()
        .map(|tool| serde_json::to_value(tool).expect("rmcp tool definition must serialize"))
        .collect();
    json!({
        "server": crate::mcp::server::SERVER_NAME,
        "version": crate::mcp::server::SERVER_VERSION,
        "tool_count": tools.len(),
        "tools": tools
    })
}
