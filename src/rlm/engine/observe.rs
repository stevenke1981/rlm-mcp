//! Workflow, trajectory, and budget ops on [`super::RlmEngine`].

use super::RlmEngine;
use crate::error::Result;
use crate::rlm::budget::SessionBudget;
use crate::rlm::workflow::workflow_guidance;
use serde_json::{json, Value};
use std::time::Instant;

impl RlmEngine {
    pub fn workflow(&self, phase: &str) -> Value {
        workflow_guidance(phase)
    }

    pub fn trajectory_get(
        &self,
        session_id: &str,
        format: &str,
        redact: bool,
        redact_patterns: &[String],
    ) -> Result<Value> {
        self.trajectory
            .get(session_id, format, redact, redact_patterns)
    }

    pub fn budget_configure(&self, config: SessionBudget) -> Result<Value> {
        self.budgets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .configure(config.clone())?;
        Ok(json!({
            "session_id": config.session_id,
            "mode": config.mode,
            "configured": true
        }))
    }

    pub fn budget_status(&self, session_id: &str) -> Value {
        // Trajectory clone does not block other sessions' record paths.
        let traj = self.trajectory.run(session_id);
        let tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let tree_refs = tasks.trees_for_session(session_id);
        self.budgets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status_report(session_id, traj.as_ref(), &tree_refs)
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
