use crate::error::{Error, Result};
use crate::rlm::budget::{self, BudgetMode};
use crate::rlm::provider::{resolve_provider, sanitize_result, ProviderResult};
use crate::rlm::session::SessionStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TaskBudget {
    pub max_depth: u32,
    pub max_fanout: u32,
    pub max_subcalls: u32,
    pub max_input_bytes: usize,
    pub max_wall_secs: u64,
}

impl Default for TaskBudget {
    fn default() -> Self {
        Self {
            max_depth: 4,
            max_fanout: 8,
            max_subcalls: 32,
            max_input_bytes: 256 * 1024,
            max_wall_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmTask {
    pub id: String,
    pub session_id: String,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub depth: u32,
    pub prompt: String,
    pub chunk_ids: Vec<String>,
    pub context_bytes: usize,
    pub provider: String,
    pub status: TaskStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub input_tokens_est: usize,
    pub output_tokens_est: usize,
    pub created_at_unix: u64,
    pub completed_at_unix: Option<u64>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTree {
    pub root_id: String,
    pub session_id: String,
    pub budget: TaskBudget,
    #[serde(default)]
    pub budget_mode: BudgetMode,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub cancel_reason: Option<String>,
    pub tasks: HashMap<String, RlmTask>,
    pub created_at_unix: u64,
}

pub struct TaskStore {
    trees: HashMap<String, TaskTree>,
}

impl TaskStore {
    pub fn new() -> Self {
        let mut trees = HashMap::new();
        if let Ok(loaded) = load_persisted_trees() {
            for tree in loaded {
                trees.insert(tree.root_id.clone(), tree);
            }
        }
        Self { trees }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &mut self,
        sessions: &SessionStore,
        session_id: &str,
        prompt: &str,
        chunk_ids: &[String],
        parent_id: Option<&str>,
        provider: &str,
        budget: Option<TaskBudget>,
        budget_mode: Option<BudgetMode>,
        execute: bool,
    ) -> Result<(RlmTask, Option<ProviderResult>)> {
        let session = sessions.get(session_id)?;
        let now = super::persistence::unix_now();

        let (root_id, parent_task, depth, budget) = if let Some(pid) = parent_id {
            let parent = self.get_task_in_any_tree(pid)?;
            if parent.session_id != session_id {
                return Err(Error::InvalidArgument(
                    "parent task session mismatch".into(),
                ));
            }
            let tree_budget = self
                .trees
                .get(&parent.root_id)
                .map(|t| t.budget)
                .unwrap_or_default();
            let tree = self
                .trees
                .get_mut(&parent.root_id)
                .ok_or_else(|| Error::Other("task tree missing".into()))?;
            budget::check_tree_wall_time(tree)?;
            enforce_budget(tree, &parent, &tree_budget, prompt, chunk_ids)?;
            let depth = parent.depth + 1;
            (parent.root_id.clone(), Some(parent), depth, tree_budget)
        } else {
            (
                Uuid::new_v4().to_string(),
                None,
                0,
                budget.unwrap_or_default(),
            )
        };

        let context = build_context(session, chunk_ids)?;
        let fingerprint = task_fingerprint(prompt, chunk_ids);

        if let Some(tree) = self.trees.get(&root_id) {
            budget::check_tree_wall_time(tree)?;
            if tree.tasks.values().any(|t| t.fingerprint == fingerprint) {
                return Err(Error::InvalidArgument(
                    "duplicate sub-task detected (same prompt + chunk_ids)".into(),
                ));
            }
        }

        let task = RlmTask {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            root_id: root_id.clone(),
            parent_id: parent_id.map(|s| s.to_string()),
            depth,
            prompt: prompt.into(),
            chunk_ids: chunk_ids.to_vec(),
            context_bytes: context.len(),
            provider: provider.into(),
            status: TaskStatus::Pending,
            result: None,
            error: None,
            input_tokens_est: 0,
            output_tokens_est: 0,
            created_at_unix: now,
            completed_at_unix: None,
            fingerprint,
        };

        let provider_result = if execute {
            Some(self.execute_task(&task, &context)?)
        } else {
            None
        };

        let mut final_task = task;
        if let Some(ref pr) = provider_result {
            final_task.status = TaskStatus::Completed;
            final_task.result = Some(pr.structured.clone());
            final_task.input_tokens_est = pr.input_tokens_est;
            final_task.output_tokens_est = pr.output_tokens_est;
            final_task.completed_at_unix = Some(super::persistence::unix_now());
        }

        if parent_task.is_none() {
            let tree = TaskTree {
                root_id: root_id.clone(),
                session_id: session_id.into(),
                budget,
                budget_mode: budget_mode.unwrap_or_default(),
                cancelled: false,
                cancel_reason: None,
                tasks: HashMap::new(),
                created_at_unix: now,
            };
            self.trees.insert(root_id.clone(), tree);
        }

        let tree = self.trees.get_mut(&root_id).unwrap();
        tree.tasks.insert(final_task.id.clone(), final_task.clone());
        persist_tree(tree)?;

        Ok((final_task, provider_result))
    }

    fn execute_task(&self, task: &RlmTask, context: &str) -> Result<ProviderResult> {
        let provider = resolve_provider(&task.provider)?;
        let mut result = sanitize_result(provider.invoke(&task.prompt, context)?);
        result.structured = sanitize_structured_task_result(&result, task);
        Ok(result)
    }

    pub fn get(&self, task_id: &str) -> Result<RlmTask> {
        self.get_task_in_any_tree(task_id)
    }

    fn get_task_in_any_tree(&self, task_id: &str) -> Result<RlmTask> {
        for tree in self.trees.values() {
            if let Some(task) = tree.tasks.get(task_id) {
                return Ok(task.clone());
            }
        }
        Err(Error::TaskNotFound(task_id.into()))
    }

    pub fn list(&self, session_id: Option<&str>, root_id: Option<&str>) -> Vec<Value> {
        self.trees
            .values()
            .filter(|t| {
                session_id.is_none_or(|sid| t.session_id == sid)
                    && root_id.is_none_or(|rid| t.root_id == rid)
            })
            .flat_map(|t| t.tasks.values())
            .map(task_summary)
            .collect()
    }

    pub fn cancel(&mut self, root_id: &str, reason: &str) -> Result<Value> {
        let tree = self
            .trees
            .get_mut(root_id)
            .ok_or_else(|| Error::TaskNotFound(root_id.into()))?;
        tree.cancelled = true;
        tree.cancel_reason = Some(reason.into());
        for task in tree.tasks.values_mut() {
            if task.status == TaskStatus::Pending || task.status == TaskStatus::Running {
                task.status = TaskStatus::Cancelled;
                task.error = Some(reason.into());
            }
        }
        persist_tree(tree)?;
        Ok(json!({
            "root_id": root_id,
            "session_id": tree.session_id,
            "cancelled": true,
            "reason": reason,
            "tasks_affected": tree.tasks.len(),
        }))
    }

    pub fn trees_for_session<'a>(&'a self, session_id: &str) -> Vec<&'a TaskTree> {
        self.trees
            .values()
            .filter(|t| t.session_id == session_id)
            .collect()
    }

    pub fn reduce(&self, root_id: &str) -> Result<Value> {
        let tree = self
            .trees
            .get(root_id)
            .ok_or_else(|| Error::TaskNotFound(root_id.into()))?;

        let tasks: Vec<_> = tree.tasks.values().collect();
        let completed: Vec<_> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .collect();
        let pending: Vec<_> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending || t.status == TaskStatus::Running)
            .map(|t| task_summary(t))
            .collect();

        let max_depth = tasks.iter().map(|t| t.depth).max().unwrap_or(0);
        let findings: Vec<Value> = completed.iter().filter_map(|t| t.result.clone()).collect();

        let total_input: usize = tasks.iter().map(|t| t.input_tokens_est).sum();
        let total_output: usize = tasks.iter().map(|t| t.output_tokens_est).sum();

        Ok(json!({
            "root_id": root_id,
            "session_id": tree.session_id,
            "task_count": tasks.len(),
            "completed_count": completed.len(),
            "max_depth": max_depth,
            "total_input_tokens_est": total_input,
            "total_output_tokens_est": total_output,
            "pending_tasks": pending,
            "findings": findings,
            "needs_recursion": !pending.is_empty(),
            "budget": tree.budget,
        }))
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

fn enforce_budget(
    tree: &TaskTree,
    parent: &RlmTask,
    budget: &TaskBudget,
    prompt: &str,
    chunk_ids: &[String],
) -> Result<()> {
    if parent.depth + 1 > budget.max_depth {
        return Err(Error::BudgetExceeded(format!(
            "max depth {} exceeded",
            budget.max_depth
        )));
    }
    let child_count = tree
        .tasks
        .values()
        .filter(|t| t.parent_id.as_deref() == Some(&parent.id))
        .count();
    if child_count as u32 >= budget.max_fanout {
        return Err(Error::BudgetExceeded(format!(
            "max fanout {} exceeded for parent {}",
            budget.max_fanout, parent.id
        )));
    }
    if tree.tasks.len() as u32 >= budget.max_subcalls {
        return Err(Error::BudgetExceeded(format!(
            "max subcalls {} exceeded",
            budget.max_subcalls
        )));
    }
    let context_len: usize = chunk_ids.len() * 64 + prompt.len();
    if context_len > budget.max_input_bytes {
        return Err(Error::BudgetExceeded(format!(
            "max input bytes {} exceeded",
            budget.max_input_bytes
        )));
    }
    Ok(())
}

fn build_context(
    session: &crate::rlm::session::ScanSession,
    chunk_ids: &[String],
) -> Result<String> {
    if chunk_ids.is_empty() {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for id in chunk_ids {
        let chunk = session
            .chunks
            .iter()
            .find(|c| c.id == *id)
            .ok_or_else(|| Error::InvalidArgument(format!("chunk not found: {id}")))?;
        parts.push(format!("--- {} ---\n{}", chunk.path, chunk.content));
    }
    Ok(parts.join("\n\n"))
}

fn task_fingerprint(prompt: &str, chunk_ids: &[String]) -> String {
    let mut ids = chunk_ids.to_vec();
    ids.sort();
    format!("{}::{}", prompt.trim(), ids.join(","))
}

fn sanitize_structured_task_result(result: &ProviderResult, task: &RlmTask) -> Value {
    json!({
        "task_id": task.id,
        "provider": result.provider,
        "output": result.output,
        "findings": result.structured.get("findings").cloned().unwrap_or_else(|| json!([])),
        "usage": result.usage,
        "cost_usd_est": result.cost_usd_est,
        "structured": result.structured,
    })
}

fn task_summary(task: &RlmTask) -> Value {
    json!({
        "id": task.id,
        "root_id": task.root_id,
        "parent_id": task.parent_id,
        "session_id": task.session_id,
        "depth": task.depth,
        "status": task.status,
        "provider": task.provider,
        "prompt_preview": &task.prompt[..task.prompt.len().min(80)],
        "chunk_ids": task.chunk_ids,
        "input_tokens_est": task.input_tokens_est,
        "output_tokens_est": task.output_tokens_est,
    })
}

fn tasks_dir() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-tasks")
}

pub fn persist_tree(tree: &TaskTree) -> Result<()> {
    let dir = tasks_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", tree.root_id));
    let tmp = dir.join(format!("{}.{}.tmp", tree.root_id, Uuid::new_v4()));
    let content = serde_json::to_string(tree)?;
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn load_persisted_trees() -> Result<Vec<TaskTree>> {
    let dir = tasks_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut trees = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        match serde_json::from_str::<TaskTree>(&content) {
            Ok(tree) => trees.push(tree),
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(trees)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rlm::session::SessionStore;
    use crate::test_lock;
    use tempfile::TempDir;

    #[test]
    fn recursive_tasks_respect_depth_budget() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let mut sessions = SessionStore::new();
        let session = sessions
            .create_from_text("fn main() {}\n", "main.rs", HashMap::new())
            .unwrap();
        let chunk_id = session.chunks[0].id.clone();

        let mut tasks = TaskStore::new();
        let budget = TaskBudget {
            max_depth: 0,
            ..Default::default()
        };

        let (root, _) = tasks
            .create(
                &sessions,
                &session.id,
                "analyze main",
                std::slice::from_ref(&chunk_id),
                None,
                "mock",
                Some(budget),
                None,
                true,
            )
            .unwrap();

        let err = tasks
            .create(
                &sessions,
                &session.id,
                "deeper analysis",
                std::slice::from_ref(&chunk_id),
                Some(&root.id),
                "mock",
                None,
                None,
                true,
            )
            .unwrap_err();
        assert!(err.to_string().contains("max depth"));

        std::env::remove_var("RLM_CACHE_DIR");
    }
}
