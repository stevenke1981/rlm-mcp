mod config;
mod env;
mod filter;
mod map;
mod persistence;
mod provider;
mod reduce;
mod session;
mod task;
mod workflow;

pub use config::RlmConfig;
pub use filter::PeekOptions;
pub use provider::{DryRunProvider, MockProvider, ProviderResult};
pub use session::*;
pub use task::{RlmTask, TaskBudget, TaskStatus};
pub use workflow::*;

use crate::error::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// RLM orchestrator: external context via scan sessions (filter → map → reduce).
pub struct RlmEngine {
    sessions: Arc<Mutex<SessionStore>>,
    tasks: Arc<Mutex<task::TaskStore>>,
}

impl RlmEngine {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(SessionStore::new())),
            tasks: Arc::new(Mutex::new(task::TaskStore::new())),
        }
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
                return Err(crate::error::Error::InvalidArgument(
                    "provide path or content".into(),
                ));
            }
        };

        Ok(json!({
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
        }))
    }

    pub fn env_info(&self, session_id: &str) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        Ok(env::env_info(session))
    }

    pub fn slice(
        &self,
        session_id: &str,
        chunk_id: &str,
        start_line: usize,
        end_line: usize,
    ) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let chunk = store.get_chunk(session_id, chunk_id)?.clone();
        Ok(env::slice_chunk(&chunk, start_line, end_line))
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

        Ok(json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "total": filtered.len(),
            "chunk_ids": page.iter().map(|c| &c.id).collect::<Vec<_>>(),
            "chunks": chunks
        }))
    }

    pub fn peek(&self, session_id: &str, opts: PeekOptions<'_>) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        Ok(filter::peek_session(session, opts))
    }

    pub fn map_plan(
        &self,
        session_id: &str,
        chunk_ids: Option<&[String]>,
        file_pattern: Option<&str>,
        batch_size: usize,
    ) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        Ok(map::map_plan(session, chunk_ids, file_pattern, batch_size))
    }

    pub fn reduce_schema(&self) -> Value {
        reduce::reduce_schema()
    }

    pub fn reduce_merge(&self, worker_outputs: &[Value]) -> Value {
        reduce::reduce_merge(worker_outputs)
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
        let sessions = self.sessions.lock().unwrap();
        let mut tasks = self.tasks.lock().unwrap();
        let (task, provider_result) = tasks.create(
            &sessions,
            session_id,
            prompt,
            chunk_ids,
            parent_task_id,
            provider,
            budget,
            execute,
        )?;
        Ok(json!({
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
        }))
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
        let tasks = self.tasks.lock().unwrap();
        tasks.reduce(root_id)
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
