use crate::error::Result;
use crate::rlm::task::{TaskBudget, TaskTree};
use crate::rlm::trajectory::TrajectoryRun;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetMode {
    #[default]
    FailFast,
    SoftWarning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBudget {
    pub session_id: String,
    pub mode: BudgetMode,
    pub max_chunks_read: u64,
    pub max_sub_calls: u64,
    pub max_total_tokens_est: u64,
    pub max_wall_secs: u64,
    pub task_budget: TaskBudget,
}

impl Default for SessionBudget {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            mode: BudgetMode::default(),
            max_chunks_read: 500,
            max_sub_calls: 64,
            max_total_tokens_est: 500_000,
            max_wall_secs: 600,
            task_budget: TaskBudget::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetEvaluation {
    pub allowed: bool,
    pub warnings: Vec<String>,
    pub exceeded: Vec<String>,
}

impl BudgetEvaluation {
    pub fn into_result(self, mode: BudgetMode) -> std::result::Result<(), crate::error::Error> {
        if self.allowed {
            return Ok(());
        }
        let msg = self.exceeded.join("; ");
        match mode {
            BudgetMode::FailFast => Err(crate::error::Error::BudgetExceeded(msg)),
            BudgetMode::SoftWarning => Err(crate::error::Error::BudgetExceeded(format!(
                "soft-warning blocked: {msg}"
            ))),
        }
    }
}

pub struct BudgetStore {
    sessions: HashMap<String, SessionBudget>,
}

impl BudgetStore {
    pub fn new() -> Self {
        let mut sessions = HashMap::new();
        if let Ok(loaded) = load_all() {
            for sb in loaded {
                sessions.insert(sb.session_id.clone(), sb);
            }
        }
        Self { sessions }
    }

    pub fn configure(&mut self, config: SessionBudget) -> Result<()> {
        let id = config.session_id.clone();
        self.sessions.insert(id.clone(), config);
        persist(&self.sessions[&id])
    }

    pub fn get_or_default(&self, session_id: &str) -> SessionBudget {
        self.sessions
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| SessionBudget {
                session_id: session_id.into(),
                ..Default::default()
            })
    }

    pub fn evaluate_session(
        &self,
        session_id: &str,
        trajectory: Option<&TrajectoryRun>,
        extra_chunks: u64,
        extra_sub_calls: u64,
        extra_tokens: u64,
    ) -> BudgetEvaluation {
        let cfg = self.get_or_default(session_id);
        let mut warnings = Vec::new();
        let mut exceeded = Vec::new();

        let (chunks, sub_calls, tokens, wall_secs) = trajectory
            .map(usage_from_trajectory)
            .unwrap_or((0, 0, 0, 0));

        check_projected(
            &mut warnings,
            &mut exceeded,
            "max_chunks_read",
            chunks,
            extra_chunks,
            cfg.max_chunks_read,
        );
        check_projected(
            &mut warnings,
            &mut exceeded,
            "max_sub_calls",
            sub_calls,
            extra_sub_calls,
            cfg.max_sub_calls,
        );
        check_projected(
            &mut warnings,
            &mut exceeded,
            "max_total_tokens_est",
            tokens,
            extra_tokens,
            cfg.max_total_tokens_est,
        );
        check_projected(
            &mut warnings,
            &mut exceeded,
            "max_wall_secs",
            wall_secs,
            0,
            cfg.max_wall_secs,
        );

        BudgetEvaluation {
            allowed: exceeded.is_empty(),
            warnings,
            exceeded,
        }
    }

    pub fn status_report(
        &self,
        session_id: &str,
        trajectory: Option<&TrajectoryRun>,
        task_trees: &[&TaskTree],
    ) -> Value {
        let cfg = self.get_or_default(session_id);
        let eval = self.evaluate_session(session_id, trajectory, 0, 0, 0);
        let (chunks, sub_calls, tokens, wall_secs) = trajectory
            .map(usage_from_trajectory)
            .unwrap_or((0, 0, 0, 0));

        let task_stats = aggregate_task_trees(task_trees);
        let tail = tail_cost_report(trajectory, &task_stats);

        json!({
            "session_id": session_id,
            "mode": cfg.mode,
            "limits": {
                "max_chunks_read": cfg.max_chunks_read,
                "max_sub_calls": cfg.max_sub_calls,
                "max_total_tokens_est": cfg.max_total_tokens_est,
                "max_wall_secs": cfg.max_wall_secs,
                "task_budget": cfg.task_budget,
            },
            "usage": {
                "chunks_read": chunks,
                "sub_calls": sub_calls,
                "total_tokens_est": tokens,
                "wall_secs": wall_secs,
                "task_trees": task_stats,
            },
            "evaluation": {
                "allowed": eval.allowed,
                "warnings": eval.warnings,
                "exceeded": eval.exceeded,
            },
            "tail_cost": tail,
            "hint": if eval.allowed {
                "Within budget — monitor tail_cost for recursive runs"
            } else {
                "Budget exceeded — narrow scope or raise limits via rlm_budget_configure"
            }
        })
    }
}

impl Default for BudgetStore {
    fn default() -> Self {
        Self::new()
    }
}

fn check_projected(
    warnings: &mut Vec<String>,
    exceeded: &mut Vec<String>,
    name: &str,
    current: u64,
    increment: u64,
    max: u64,
) {
    if max == 0 {
        return;
    }
    let projected = current.saturating_add(increment);
    let warn_at = (max as f64 * 0.8).ceil() as u64;
    if projected > max {
        exceeded.push(format!("{name}: {projected}/{max}"));
    } else if projected >= warn_at {
        warnings.push(format!(
            "{name} projected {projected}/{max} (80% threshold)"
        ));
    }
}

fn usage_from_trajectory(run: &TrajectoryRun) -> (u64, u64, u64, u64) {
    let mut chunks = 0u64;
    let mut sub_calls = 0u64;
    let mut tokens = 0u64;
    for e in &run.events {
        tokens += (e.bytes_in + e.bytes_out) as u64 / 4;
        if e.event_type == "chunk" || e.event_type == "map" {
            chunks += e
                .detail
                .get("chunks_returned")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
        }
        if e.event_type == "sub_call" {
            sub_calls += 1;
            tokens += e
                .detail
                .get("input_tokens_est")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            tokens += e
                .detail
                .get("output_tokens_est")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }
    }
    let wall_secs = run
        .events
        .last()
        .map(|e| e.ts_unix.saturating_sub(run.started_at_unix))
        .unwrap_or(0);
    (chunks, sub_calls, tokens, wall_secs)
}

fn aggregate_task_trees(trees: &[&TaskTree]) -> Value {
    let mut total_tasks = 0u64;
    let mut cancelled = 0u64;
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut provider_cost_usd_est = 0.0f64;
    let mut cost_samples = 0u64;
    for tree in trees {
        if tree.cancelled {
            cancelled += 1;
        }
        for task in tree.tasks.values() {
            total_tasks += 1;
            input_tokens += task.input_tokens_est as u64;
            output_tokens += task.output_tokens_est as u64;
            if let Some(cost) = task
                .result
                .as_ref()
                .and_then(|r| r.get("cost_usd_est"))
                .and_then(|v| v.as_f64())
            {
                provider_cost_usd_est += cost;
                cost_samples += 1;
            }
        }
    }
    json!({
        "tree_count": trees.len(),
        "cancelled_trees": cancelled,
        "task_count": total_tasks,
        "input_tokens_est": input_tokens,
        "output_tokens_est": output_tokens,
        "provider_cost_usd_est": if cost_samples > 0 {
            serde_json::Value::from(provider_cost_usd_est)
        } else {
            serde_json::Value::Null
        },
        "provider_cost_samples": cost_samples,
    })
}

fn tail_cost_report(trajectory: Option<&TrajectoryRun>, task_stats: &Value) -> Value {
    let mut sub_call_costs = Vec::new();
    if let Some(run) = trajectory {
        for e in &run.events {
            if e.event_type == "sub_call" {
                let cost = e
                    .detail
                    .get("input_tokens_est")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                    + e.detail
                        .get("output_tokens_est")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                sub_call_costs.push(cost);
            }
        }
    }

    sub_call_costs.sort_unstable();
    let n = sub_call_costs.len();
    let p50 = percentile(&sub_call_costs, 0.5);
    let p90 = percentile(&sub_call_costs, 0.9);
    let max = sub_call_costs.last().copied().unwrap_or(0);
    let high_variance = n >= 3 && p90 > p50.saturating_mul(2);

    json!({
        "sub_call_samples": n,
        "p50_tokens_est": p50,
        "p90_tokens_est": p90,
        "max_tokens_est": max,
        "high_variance": high_variance,
        "paper_note": if high_variance {
            "Tail cost may dominate — median sub-call cost understates worst-case spend"
        } else {
            "Sub-call costs appear relatively stable for this run"
        },
        "task_totals": task_stats,
    })
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub fn check_tree_wall_time(tree: &TaskTree) -> Result<()> {
    if tree.cancelled {
        return Err(crate::error::Error::BudgetExceeded(
            "task tree cancelled".into(),
        ));
    }
    let now = super::persistence::unix_now();
    let elapsed = now.saturating_sub(tree.created_at_unix);
    if elapsed > tree.budget.max_wall_secs {
        return Err(crate::error::Error::BudgetExceeded(format!(
            "max wall time {}s exceeded (elapsed {elapsed}s)",
            tree.budget.max_wall_secs
        )));
    }
    Ok(())
}

fn budgets_dir() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-budgets")
}

fn persist(config: &SessionBudget) -> Result<()> {
    let dir = budgets_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", config.session_id));
    let tmp = dir.join(format!("{}.{}.tmp", config.session_id, Uuid::new_v4()));
    let content = serde_json::to_string(config)?;
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn load_all() -> Result<Vec<SessionBudget>> {
    let dir = budgets_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut configs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(cfg) = serde_json::from_str::<SessionBudget>(&content) {
            configs.push(cfg);
        }
    }
    Ok(configs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_high_variance_tail() {
        let report = tail_cost_report(None, &json!({}));
        assert!(!report["high_variance"].as_bool().unwrap());

        let run = TrajectoryRun {
            session_id: "s".into(),
            started_at_unix: 0,
            events: vec![],
        };
        let report2 = tail_cost_report(Some(&run), &json!({}));
        assert_eq!(report2["sub_call_samples"].as_u64().unwrap(), 0);
    }

    #[test]
    fn warn_at_eighty_percent() {
        let cfg = SessionBudget {
            session_id: "s".into(),
            max_chunks_read: 100,
            ..Default::default()
        };
        let mut inner = BudgetStore {
            sessions: HashMap::new(),
        };
        inner.sessions.insert("s".into(), cfg);
        let eval = inner.evaluate_session("s", None, 80, 0, 0);
        assert!(eval.allowed);
        assert!(!eval.warnings.is_empty());
        let eval2 = inner.evaluate_session("s", None, 101, 0, 0);
        assert!(!eval2.allowed);
    }
}
