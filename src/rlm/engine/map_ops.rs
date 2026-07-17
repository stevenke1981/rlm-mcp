//! Map / reduce coordination on [`super::RlmEngine`].

use super::RlmEngine;
use crate::error::Result;
use crate::rlm::map;
use crate::rlm::map_ledger;
use crate::rlm::reduce;
use crate::rlm::trajectory;
use serde_json::{json, Value};
use std::time::Instant;

impl RlmEngine {
    pub fn map_plan(
        &self,
        session_id: &str,
        chunk_ids: Option<&[String]>,
        file_pattern: Option<&str>,
        batch_size: usize,
    ) -> Result<Value> {
        let started = Instant::now();
        let session = self.session_snapshot(session_id)?;
        let mut out = map::map_plan(&session, chunk_ids, file_pattern, batch_size);
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
}
