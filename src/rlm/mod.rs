mod config;
mod env;
mod filter;
mod map;
mod persistence;
mod provider;
mod reduce;
mod session;
mod task;
mod trajectory;
mod workflow;

pub use config::RlmConfig;
pub use filter::PeekOptions;
pub use provider::{DryRunProvider, MockProvider, ProviderResult};
pub use session::*;
pub use task::{RlmTask, TaskBudget, TaskStatus};
pub use workflow::*;

use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// RLM orchestrator: external context via scan sessions (filter → map → reduce).
pub struct RlmEngine {
    sessions: Arc<Mutex<SessionStore>>,
    tasks: Arc<Mutex<task::TaskStore>>,
    trajectory: Arc<Mutex<trajectory::TrajectoryStore>>,
}

impl RlmEngine {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(SessionStore::new())),
            tasks: Arc::new(Mutex::new(task::TaskStore::new())),
            trajectory: Arc::new(Mutex::new(trajectory::TrajectoryStore::new())),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        session_id: &str,
        event_type: &str,
        task_id: Option<&str>,
        detail: Value,
        bytes_in: usize,
        bytes_out: usize,
        started: Instant,
    ) {
        self.trajectory.lock().unwrap().record(
            session_id, event_type, task_id, detail, bytes_in, bytes_out, started,
        );
    }

    pub fn workflow(&self, phase: &str) -> Value {
        workflow_guidance(phase)
    }

    pub fn scan(
        &self,
        path: Option<&str>,
        content: Option<&str>,
        virtual_path: Option<&str>,
        variable_name: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let mut store = self.sessions.lock().unwrap();
        let session = match (path, content) {
            (Some(p), None) | (Some(p), Some(_)) => store.create_from_path(p)?,
            (None, Some(text)) => {
                let vp = virtual_path.unwrap_or("inline.txt");
                let mut vars = HashMap::new();
                if let Some(name) = variable_name {
                    vars.insert(name.to_string(), text.to_string());
                }
                store.create_from_text(text, vp, vars)?
            }
            (None, None) => {
                return Err(Error::InvalidArgument("provide path or content".into()));
            }
        };

        let out = json!({
            "session_id": session.id,
            "root_path": session.root_path,
            "source_kind": session.source_kind,
            "file_count": session.files_scanned,
            "chunk_count": session.chunks.len(),
            "total_bytes": session.total_bytes,
            "files_scanned": session.files_scanned,
            "files_skipped": session.files_skipped,
            "skip_reasons": session.skip_reasons,
            "variables": session.variables.keys().collect::<Vec<_>>(),
            "created_at_unix": session.created_at_unix,
            "expires_at_unix": session.expires_at_unix,
            "hint": "Use rlm_env_info to inspect, rlm_peek to filter, rlm_chunk to read"
        });
        self.record(
            &session.id,
            "scan",
            None,
            json!({
                "source_kind": session.source_kind,
                "chunk_count": session.chunks.len(),
                "total_bytes": session.total_bytes,
                "files_scanned": session.files_scanned,
            }),
            path.map(|p| p.len()).unwrap_or(0) + content.map(|c| c.len()).unwrap_or(0),
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn env_info(&self, session_id: &str) -> Result<Value> {
        let started = Instant::now();
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let out = env::env_info(session);
        self.record(
            session_id,
            "load",
            None,
            json!({
                "chunk_count": out["chunk_count"],
                "file_count": out["file_count"],
                "context_length_bytes": out["context_length_bytes"],
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn slice(
        &self,
        session_id: &str,
        chunk_id: &str,
        start_line: usize,
        end_line: usize,
    ) -> Result<Value> {
        let started = Instant::now();
        let store = self.sessions.lock().unwrap();
        let chunk = store.get_chunk(session_id, chunk_id)?.clone();
        let out = env::slice_chunk(&chunk, start_line, end_line);
        self.record(
            session_id,
            "slice",
            None,
            json!({
                "chunk_id": chunk_id,
                "start_line": start_line,
                "end_line": end_line,
                "line_count": out["line_count"],
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn chunk(
        &self,
        session_id: &str,
        file_pattern: Option<&str>,
        chunk_ids: Option<&[String]>,
        offset: usize,
        limit: usize,
        _include_metadata: bool,
    ) -> Result<Value> {
        let started = Instant::now();
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let filtered: Vec<_> = session
            .chunks
            .iter()
            .filter(|c| {
                if let Some(ids) = chunk_ids {
                    if !ids.contains(&c.id) {
                        return false;
                    }
                }
                file_pattern.is_none_or(|pat| {
                    c.path.contains(pat) || c.path.ends_with(pat) || glob_match(pat, &c.path)
                })
            })
            .collect();

        let page: Vec<_> = filtered.iter().skip(offset).take(limit).collect();
        let chunks: Vec<Value> = page
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "path": c.path,
                    "offset": c.offset,
                    "line_count": c.line_count,
                    "content": c.content
                })
            })
            .collect();

        let out = json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "total": filtered.len(),
            "chunk_ids": page.iter().map(|c| &c.id).collect::<Vec<_>>(),
            "chunks": chunks
        });
        self.record(
            session_id,
            "chunk",
            None,
            json!({
                "offset": offset,
                "limit": limit,
                "chunks_returned": page.len(),
                "chunk_ids": page.iter().map(|c| &c.id).collect::<Vec<_>>(),
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn peek(&self, session_id: &str, opts: PeekOptions<'_>) -> Result<Value> {
        let started = Instant::now();
        let query_len = opts.query.map(|q| q.len()).unwrap_or(0);
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let out = filter::peek_session(session, opts);
        self.record(
            session_id,
            "peek",
            None,
            json!({
                "query": out.get("query"),
                "returned": out.get("returned"),
                "total_match_lines": out.get("total_match_lines"),
                "truncated": out.get("truncated"),
            }),
            query_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn map_plan(
        &self,
        session_id: &str,
        chunk_ids: Option<&[String]>,
        file_pattern: Option<&str>,
        batch_size: usize,
    ) -> Result<Value> {
        let started = Instant::now();
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let out = map::map_plan(session, chunk_ids, file_pattern, batch_size);
        self.record(
            session_id,
            "map",
            None,
            json!({
                "total_chunks": out.get("total_chunks"),
                "batch_count": out.get("batches").and_then(|b| b.as_array()).map(|a| a.len()),
                "batch_size": batch_size,
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn reduce_schema(&self) -> Value {
        reduce::reduce_schema()
    }

    pub fn reduce_merge(&self, worker_outputs: &[Value]) -> Result<Value> {
        let started = Instant::now();
        let out = reduce::reduce_merge(worker_outputs);
        let session_id = worker_outputs
            .first()
            .and_then(|w| w.get("session_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        self.record(
            session_id,
            "reduce",
            None,
            json!({
                "finding_count": out.get("finding_count"),
                "needs_recursion": out.get("needs_recursion"),
                "worker_count": worker_outputs.len(),
            }),
            trajectory::detail_size(&json!(worker_outputs)),
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn session_list(&self) -> Value {
        let store = self.sessions.lock().unwrap();
        json!({ "sessions": store.list() })
    }

    pub fn session_delete(&self, session_id: &str) -> Result<Value> {
        self.sessions.lock().unwrap().delete(session_id)?;
        Ok(json!({ "session_id": session_id, "deleted": true }))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn task_create(
        &self,
        session_id: &str,
        prompt: &str,
        chunk_ids: &[String],
        parent_task_id: Option<&str>,
        provider: &str,
        budget: Option<TaskBudget>,
        execute: bool,
    ) -> Result<Value> {
        let started = Instant::now();
        let sessions = self.sessions.lock().unwrap();
        let mut tasks = self.tasks.lock().unwrap();
        let result = tasks.create(
            &sessions,
            session_id,
            prompt,
            chunk_ids,
            parent_task_id,
            provider,
            budget,
            execute,
        );
        match result {
            Ok((task, provider_result)) => {
                let out = json!({
                    "task_id": task.id,
                    "root_id": task.root_id,
                    "parent_id": task.parent_id,
                    "session_id": task.session_id,
                    "depth": task.depth,
                    "status": task.status,
                    "provider": task.provider,
                    "chunk_ids": task.chunk_ids,
                    "context_bytes": task.context_bytes,
                    "input_tokens_est": task.input_tokens_est,
                    "output_tokens_est": task.output_tokens_est,
                    "result": task.result,
                    "provider_result": provider_result,
                    "hint": "Use rlm_task_list / rlm_task_result; rlm_task_reduce on root_id"
                });
                self.record(
                    session_id,
                    "sub_call",
                    Some(&task.id),
                    json!({
                        "root_id": task.root_id,
                        "depth": task.depth,
                        "provider": task.provider,
                        "input_tokens_est": task.input_tokens_est,
                        "output_tokens_est": task.output_tokens_est,
                        "chunk_ids": task.chunk_ids,
                    }),
                    prompt.len(),
                    trajectory::detail_size(&out),
                    started,
                );
                Ok(out)
            }
            Err(Error::BudgetExceeded(msg)) => {
                self.record(
                    session_id,
                    "budget",
                    parent_task_id,
                    json!({ "error": msg }),
                    prompt.len(),
                    0,
                    started,
                );
                Err(Error::BudgetExceeded(msg))
            }
            Err(e) => {
                self.record(
                    session_id,
                    "error",
                    parent_task_id,
                    json!({ "error": e.to_string(), "operation": "task_create" }),
                    prompt.len(),
                    0,
                    started,
                );
                Err(e)
            }
        }
    }

    pub fn task_list(&self, session_id: Option<&str>, root_id: Option<&str>) -> Value {
        let tasks = self.tasks.lock().unwrap();
        json!({ "tasks": tasks.list(session_id, root_id) })
    }

    pub fn task_result(&self, task_id: &str) -> Result<Value> {
        let tasks = self.tasks.lock().unwrap();
        let task = tasks.get(task_id)?;
        Ok(json!({
            "task_id": task.id,
            "root_id": task.root_id,
            "parent_id": task.parent_id,
            "session_id": task.session_id,
            "depth": task.depth,
            "status": task.status,
            "provider": task.provider,
            "prompt": task.prompt,
            "chunk_ids": task.chunk_ids,
            "context_bytes": task.context_bytes,
            "input_tokens_est": task.input_tokens_est,
            "output_tokens_est": task.output_tokens_est,
            "result": task.result,
            "error": task.error,
            "created_at_unix": task.created_at_unix,
            "completed_at_unix": task.completed_at_unix,
        }))
    }

    pub fn task_reduce(&self, root_id: &str) -> Result<Value> {
        let started = Instant::now();
        let tasks = self.tasks.lock().unwrap();
        let out = tasks.reduce(root_id)?;
        let session_id = out
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        self.record(
            session_id,
            "reduce",
            Some(root_id),
            json!({
                "task_count": out.get("task_count"),
                "completed_count": out.get("completed_count"),
                "total_input_tokens_est": out.get("total_input_tokens_est"),
                "total_output_tokens_est": out.get("total_output_tokens_est"),
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn trajectory_get(
        &self,
        session_id: &str,
        format: &str,
        redact: bool,
        redact_patterns: &[String],
    ) -> Result<Value> {
        self.trajectory
            .lock()
            .unwrap()
            .get(session_id, format, redact, redact_patterns)
    }

    pub fn trajectory_record_final(
        &self,
        session_id: &str,
        answer_summary: &str,
        evidence_count: usize,
    ) -> Value {
        let started = Instant::now();
        let detail = json!({
            "answer_preview": &answer_summary[..answer_summary.len().min(200)],
            "evidence_count": evidence_count,
        });
        self.record(
            session_id,
            "final_answer",
            None,
            detail.clone(),
            0,
            answer_summary.len(),
            started,
        );
        json!({
            "session_id": session_id,
            "recorded": true,
            "event_type": "final_answer"
        })
    }
}

impl Default for RlmEngine {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn glob_match(pattern: &str, path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    simple_glob(pattern, file_name) || simple_glob(pattern, path)
}

fn simple_glob(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text.contains(pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut start = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text.starts_with(part) {
                return false;
            }
            start = part.len();
        } else if i == parts.len() - 1 {
            if !text[start..].ends_with(part) {
                return false;
            }
        } else if let Some(pos) = text[start..].find(part) {
            start += pos + part.len();
        } else {
            return false;
        }
    }
    true
}
