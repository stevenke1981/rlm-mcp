//! RLM orchestrator split by concern (session / map-reduce / task / observe).
//!
//! Public surface remains `crate::rlm::RlmEngine` via re-export from `rlm/mod.rs`.
//!
//! Concurrency:
//! - `sessions` is an [`RwLock`]: concurrent readers (peek/chunk after hydrate)
//!   share the map; writers (scan/delete) take exclusive access.
//! - `trajectory` is interior-mutable with **per-session** locks (no outer mutex).
//! - `tasks` / `budgets` remain process-wide mutexes (lower traffic).

mod map_ops;
mod observe;
mod session_ops;
mod task_ops;

use crate::error::Result;
use crate::rlm::budget;
use crate::rlm::session::{ScanSession, SessionStore};
use crate::rlm::task;
use crate::rlm::trajectory::TrajectoryStore;
use serde_json::Value;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

/// RLM orchestrator: external context via scan sessions (filter → map → reduce).
pub struct RlmEngine {
    pub(crate) sessions: Arc<RwLock<SessionStore>>,
    pub(crate) tasks: Arc<Mutex<task::TaskStore>>,
    /// Per-session fine-grained locking inside the store (see `trajectory` module).
    pub(crate) trajectory: Arc<TrajectoryStore>,
    pub(crate) budgets: Arc<Mutex<budget::BudgetStore>>,
}

impl RlmEngine {
    pub fn new() -> Self {
        let _ = crate::project::init_cache();
        Self {
            sessions: Arc::new(RwLock::new(SessionStore::new())),
            tasks: Arc::new(Mutex::new(task::TaskStore::new())),
            trajectory: Arc::new(TrajectoryStore::new()),
            budgets: Arc::new(Mutex::new(budget::BudgetStore::new())),
        }
    }

    /// Snapshot a session for read-only work without holding the store lock.
    ///
    /// Tries a shared read first; hydrates under a write lock only on miss.
    /// Lazy chunk bodies stay on disk — cloning is metadata-cheap.
    pub(crate) fn session_snapshot(&self, session_id: &str) -> Result<ScanSession> {
        {
            let store = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            if let Ok(session) = store.get(session_id) {
                return Ok(session.clone());
            }
        }
        let mut store = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        store.get_or_hydrate(session_id)?;
        store.get(session_id).cloned()
    }

    pub(crate) fn ensure_session_budget(
        &self,
        session_id: &str,
        extra_chunks: u64,
        extra_sub_calls: u64,
        extra_tokens: u64,
    ) -> Result<budget::BudgetEvaluation> {
        let traj = self.trajectory.run(session_id);
        let store = self.budgets.lock().unwrap_or_else(|e| e.into_inner());
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
        self.trajectory.record(
            session_id, event_type, task_id, detail, bytes_in, bytes_out, started,
        );
    }
}

impl Default for RlmEngine {
    fn default() -> Self {
        Self::new()
    }
}
