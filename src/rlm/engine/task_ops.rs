//! Recursive task / provider ops on [`super::RlmEngine`].

use super::RlmEngine;
use crate::error::{Error, Result};
use crate::rlm::budget::BudgetMode;
use crate::rlm::task::TaskBudget;
use crate::rlm::trajectory;
use serde_json::{json, Value};
use std::time::Instant;

impl RlmEngine {
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
        // Hydrate under write, create under short read+tasks locks, then release before trajectory.
        {
            let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            sessions.hydrate(session_id)?;
        }
        let result = {
            let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            tasks.create(
                &sessions,
                session_id,
                prompt,
                chunk_ids,
                parent_task_id,
                provider,
                budget,
                budget_mode,
                execute,
            )
        };
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
}
