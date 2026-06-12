mod artifacts;
mod bm25;
mod budget;
mod config;
mod env;
mod filter;
mod map;
mod map_ledger;
mod persistence;
mod provider;
mod reduce;
mod repl;
mod safety;
mod session;
mod task;
mod trajectory;
mod transform;
mod workflow;

pub use budget::{BudgetMode, SessionBudget};
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
    budgets: Arc<Mutex<budget::BudgetStore>>,
}

impl RlmEngine {
    pub fn new() -> Self {
        let _ = crate::project::init_cache();
        Self {
            sessions: Arc::new(Mutex::new(SessionStore::new())),
            tasks: Arc::new(Mutex::new(task::TaskStore::new())),
            trajectory: Arc::new(Mutex::new(trajectory::TrajectoryStore::new())),
            budgets: Arc::new(Mutex::new(budget::BudgetStore::new())),
        }
    }

    fn ensure_session_budget(
        &self,
        session_id: &str,
        extra_chunks: u64,
        extra_sub_calls: u64,
        extra_tokens: u64,
    ) -> Result<budget::BudgetEvaluation> {
        let traj = self.trajectory.lock().unwrap().run(session_id);
        let store = self.budgets.lock().unwrap();
        let cfg = store.get_or_default(session_id);
        let eval = store.evaluate_session(
            session_id,
            traj.as_ref(),
            extra_chunks,
            extra_sub_calls,
            extra_tokens,
        );
        if !eval.allowed {
            eval.clone().into_result(cfg.mode)?;
        }
        Ok(eval)
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
        let mut store = self.sessions.lock().unwrap();
        let session = store.get_or_hydrate(session_id)?;
        let mut out = env::env_info(session);
        if let Some(obj) = out.as_object_mut() {
            obj.insert("repl".into(), repl::list_backends());
        }
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
        let mut store = self.sessions.lock().unwrap();
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

    fn resolve_text_input(
        &self,
        session_id: &str,
        chunk_id: Option<&str>,
        artifact_name: Option<&str>,
        content: Option<&str>,
    ) -> Result<String> {
        if let Some(text) = content {
            return Ok(text.to_string());
        }
        if let Some(name) = artifact_name {
            let read = artifacts::read_artifact(session_id, name, None, None)?;
            return read
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| Error::Other("artifact read missing content".into()));
        }
        if let Some(id) = chunk_id {
            let mut store = self.sessions.lock().unwrap();
            let chunk = store.get_chunk(session_id, id)?.clone();
            return Ok(chunk.content);
        }
        Err(Error::InvalidArgument(
            "provide content, artifact_name, or chunk_id".into(),
        ))
    }

    pub fn transform(
        &self,
        session_id: &str,
        operation: &str,
        params: &Value,
        chunk_id: Option<&str>,
        artifact_name: Option<&str>,
        content: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let input = self.resolve_text_input(session_id, chunk_id, artifact_name, content)?;
        let input_len = input.len();
        let out =
            repl::ReplBackend::execute_transform(&repl::safe_backend(), &input, operation, params)?;
        self.record(
            session_id,
            "transform",
            None,
            json!({
                "backend": "safe_builtin",
                "operation": operation,
                "input_chars": input_len,
                "output_chars": out.get("output_chars"),
                "truncated": out.get("truncated"),
            }),
            input_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn transform_operations(&self) -> Value {
        transform::supported_operations()
    }

    pub fn repl_info(&self) -> Value {
        repl::list_backends()
    }

    pub fn repl_execute(
        &self,
        session_id: &str,
        code: &str,
        language: Option<&str>,
        backend: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let backend_id = backend
            .and_then(repl::ReplBackendId::parse)
            .unwrap_or(repl::ReplBackendId::Command);

        if backend_id == repl::ReplBackendId::SafeBuiltin {
            return Err(Error::InvalidArgument(
                "repl_execute requires an executable backend (command or python)".into(),
            ));
        }

        let exec_backend: Box<dyn repl::ReplBackend> = match backend_id {
            repl::ReplBackendId::SafeBuiltin => Box::new(repl::SafeBuiltinBackend),
            repl::ReplBackendId::Command => Box::new(repl::CommandSandboxBackend::new(
                repl::SandboxLimits::from_env(),
            )),
            repl::ReplBackendId::Python => {
                return Err(Error::InvalidArgument(
                    "python REPL backend is not implemented; use backend=command".into(),
                ));
            }
        };

        let lang = language.unwrap_or("text");
        let code_len = code.len();
        let out = exec_backend.execute_code(session_id, code, lang)?;
        let wall_ms = started.elapsed().as_millis() as u64;

        self.record(
            session_id,
            "repl_exec",
            None,
            json!({
                "backend": exec_backend.name(),
                "language": lang,
                "input_bytes": code_len,
                "output_bytes": out.get("output_chars"),
                "wall_ms": wall_ms,
                "truncated": out.get("truncated"),
                "audit": out.get("audit"),
            }),
            code_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn artifact_write(
        &self,
        session_id: &str,
        name: &str,
        content: Option<&str>,
        source_chunk_id: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let body = if let Some(text) = content {
            text.to_string()
        } else if let Some(chunk_id) = source_chunk_id {
            let mut store = self.sessions.lock().unwrap();
            store.get_chunk(session_id, chunk_id)?.content.clone()
        } else {
            return Err(Error::InvalidArgument(
                "provide content or source_chunk_id".into(),
            ));
        };
        let byte_len = body.len();
        let out = artifacts::write_artifact(session_id, name, &body)?;
        self.record(
            session_id,
            "artifact_write",
            None,
            json!({
                "name": out.get("name"),
                "bytes": byte_len,
            }),
            byte_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn artifact_read(
        &self,
        session_id: &str,
        name: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<Value> {
        let started = Instant::now();
        let out = artifacts::read_artifact(session_id, name, start_line, end_line)?;
        let bytes = out.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        self.record(
            session_id,
            "artifact_read",
            None,
            json!({
                "name": name,
                "bytes": bytes,
                "start_line": start_line,
                "end_line": end_line,
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
        let budget_eval = self.ensure_session_budget(session_id, limit as u64, 0, 0)?;
        let mut store = self.sessions.lock().unwrap();
        let session = store.get_or_hydrate(session_id)?;
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
        let max_chunk_bytes = safety::max_chunk_output_bytes();
        let mut any_truncated = false;
        let chunks: Vec<Value> = page
            .iter()
            .map(|c| {
                let (content, truncated) = safety::truncate_chunk_content(&c.content);
                if truncated {
                    any_truncated = true;
                }
                json!({
                    "id": c.id,
                    "path": c.path,
                    "offset": c.offset,
                    "line_count": c.line_count,
                    "content": content,
                    "truncated": truncated,
                    "max_chunk_bytes": max_chunk_bytes
                })
            })
            .collect();

        let mut out = json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "total": filtered.len(),
            "chunk_ids": page.iter().map(|c| &c.id).collect::<Vec<_>>(),
            "chunks": chunks,
            "max_chunk_bytes": max_chunk_bytes,
            "any_truncated": any_truncated
        });
        if !budget_eval.warnings.is_empty() {
            out["budget_warnings"] = json!(budget_eval.warnings);
        }
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
        let mut store = self.sessions.lock().unwrap();
        let session = store.get_or_hydrate(session_id)?;
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
        let mut store = self.sessions.lock().unwrap();
        let session = store.get_or_hydrate(session_id)?;
        let mut out = map::map_plan(session, chunk_ids, file_pattern, batch_size);
        let batches = out
            .get("batches")
            .and_then(|b| b.as_array())
            .cloned()
            .unwrap_or_default();
        let plan_id = map_ledger::create_and_persist(session_id, batch_size, &batches)?;
        if let Some(obj) = out.as_object_mut() {
            obj.insert("plan_id".into(), json!(plan_id));
        }
        self.record(
            session_id,
            "map",
            None,
            json!({
                "plan_id": plan_id,
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

    pub fn map_claim(
        &self,
        plan_id: &str,
        worker_id: &str,
        batch_id: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let out = map_ledger::claim(plan_id, worker_id, batch_id)?;
        let session_id = out
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        self.record(
            session_id,
            "map",
            None,
            json!({
                "plan_id": plan_id,
                "worker_id": worker_id,
                "batch_id": out.get("batch_id"),
                "status": out.get("status"),
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn map_complete(
        &self,
        plan_id: &str,
        worker_id: &str,
        batch_id: &str,
        output: Value,
    ) -> Result<Value> {
        let started = Instant::now();
        let plan = map_ledger::load_plan(plan_id)?;
        let out = map_ledger::complete(plan_id, worker_id, batch_id, output)?;
        self.record(
            &plan.session_id,
            "map",
            None,
            json!({
                "plan_id": plan_id,
                "worker_id": worker_id,
                "batch_id": batch_id,
                "all_complete": out.get("all_complete"),
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

    pub fn session_cleanup(&self) -> Result<Value> {
        let report = self.sessions.lock().unwrap().cleanup_expired()?;
        Ok(json!({
            "removed_count": report.removed_count,
            "removed_ids": report.removed_ids,
        }))
    }

    pub fn session_export(&self, session_id: &str) -> Result<Value> {
        let session = self.sessions.lock().unwrap().export(session_id)?;
        Ok(json!({
            "session_id": session.id,
            "revision": session.revision,
            "session": serde_json::to_value(session)?,
        }))
    }

    pub fn session_import(&self, session: ScanSession, preserve_id: bool) -> Result<Value> {
        let imported = self
            .sessions
            .lock()
            .unwrap()
            .import_session(session, preserve_id)?;
        Ok(json!({
            "session_id": imported.id,
            "revision": imported.revision,
            "chunk_count": imported.chunks.len(),
            "total_bytes": imported.total_bytes,
        }))
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
        budget_mode: Option<BudgetMode>,
        execute: bool,
    ) -> Result<Value> {
        let started = Instant::now();
        let est_tokens = (prompt.len() + chunk_ids.len() * 64) as u64 / 4;
        let budget_eval = self.ensure_session_budget(session_id, 0, 1, est_tokens)?;
        let mut sessions = self.sessions.lock().unwrap();
        sessions.hydrate(session_id)?;
        let mut tasks = self.tasks.lock().unwrap();
        let result = tasks.create(
            &sessions,
            session_id,
            prompt,
            chunk_ids,
            parent_task_id,
            provider,
            budget,
            budget_mode,
            execute,
        );
        match result {
            Ok((task, provider_result)) => {
                let mut out = json!({
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
                if !budget_eval.warnings.is_empty() {
                    out["budget_warnings"] = json!(budget_eval.warnings);
                }
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

    pub fn budget_configure(&self, config: SessionBudget) -> Result<Value> {
        self.budgets.lock().unwrap().configure(config.clone())?;
        Ok(json!({
            "session_id": config.session_id,
            "mode": config.mode,
            "configured": true
        }))
    }

    pub fn budget_status(&self, session_id: &str) -> Value {
        let traj = self.trajectory.lock().unwrap().run(session_id);
        let tasks = self.tasks.lock().unwrap();
        let tree_refs = tasks.trees_for_session(session_id);
        self.budgets
            .lock()
            .unwrap()
            .status_report(session_id, traj.as_ref(), &tree_refs)
    }

    pub fn task_cancel(&self, root_id: &str, reason: &str) -> Result<Value> {
        let started = Instant::now();
        let out = self.tasks.lock().unwrap().cancel(root_id, reason)?;
        let session_id = out
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        self.record(
            session_id,
            "cancel",
            Some(root_id),
            json!({ "reason": reason, "root_id": root_id }),
            0,
            0,
            started,
        );
        Ok(out)
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
