//! RLM orchestrator split by concern (session / map-reduce / task / observe).
//!
//! Public surface remains `crate::rlm::RlmEngine` via re-export from `rlm/mod.rs`.

mod map_ops;
mod observe;
mod session_ops;
mod task_ops;

use crate::error::Result;
use crate::rlm::budget;
use crate::rlm::session::SessionStore;
use crate::rlm::task;
use crate::rlm::trajectory;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// RLM orchestrator: external context via scan sessions (filter → map → reduce).
pub struct RlmEngine {
    pub(crate) sessions: Arc<Mutex<SessionStore>>,
    pub(crate) tasks: Arc<Mutex<task::TaskStore>>,
    pub(crate) trajectory: Arc<Mutex<trajectory::TrajectoryStore>>,
    pub(crate) budgets: Arc<Mutex<budget::BudgetStore>>,
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

    pub(crate) fn ensure_session_budget(
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
    pub(crate) fn record(
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
}

impl Default for RlmEngine {
    fn default() -> Self {
        Self::new()
    }
}
