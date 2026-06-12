use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const LOCK_RETRIES: u32 = 8;
const LOCK_SLEEP_MS: u64 = 25;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Pending,
    Claimed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapBatchState {
    pub batch_id: String,
    pub chunk_ids: Vec<String>,
    pub status: BatchStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapPlan {
    pub plan_id: String,
    pub session_id: String,
    pub created_at_unix: u64,
    pub batch_size: usize,
    pub batches: Vec<MapBatchState>,
}

pub fn plans_dir() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-map-plans")
}

fn plan_file_path(plan_id: &str) -> PathBuf {
    plans_dir().join(format!("{plan_id}.json"))
}

struct PlanLock {
    path: PathBuf,
}

impl PlanLock {
    fn acquire(plan_id: &str) -> Result<Self> {
        let dir = plans_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{plan_id}.lock"));
        for attempt in 0..LOCK_RETRIES {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = writeln!(
                        file,
                        "pid={} ts={}",
                        std::process::id(),
                        super::persistence::unix_now()
                    );
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt + 1 == LOCK_RETRIES {
                        return Err(Error::Other(format!(
                            "map plan lock busy: {plan_id} (another writer active)"
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(LOCK_SLEEP_MS));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Other(format!("map plan lock busy: {plan_id}")))
    }
}

impl Drop for PlanLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn atomic_write_json(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("plan path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Other("invalid plan path".into()))?;
    let tmp = parent.join(format!("{file_name}.{}.tmp", Uuid::new_v4()));
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_e) if cfg!(windows) => {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
            std::fs::rename(&tmp, path)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

pub fn persist_plan(plan: &MapPlan) -> Result<()> {
    let path = plan_file_path(&plan.plan_id);
    let content = serde_json::to_string(plan)?;
    atomic_write_json(&path, &content)
}

pub fn load_plan(plan_id: &str) -> Result<MapPlan> {
    let path = plan_file_path(plan_id);
    if !path.exists() {
        return Err(Error::Other(format!("map plan not found: {plan_id}")));
    }
    let content = std::fs::read_to_string(&path)?;
    serde_json::from_str(&content).map_err(Into::into)
}

pub fn create_and_persist(
    session_id: &str,
    batch_size: usize,
    batches_value: &[Value],
) -> Result<String> {
    let plan_id = Uuid::new_v4().to_string();
    let batches: Vec<MapBatchState> = batches_value
        .iter()
        .map(|b| {
            let batch_id = b["batch_id"].as_str().unwrap_or("batch-0").to_string();
            let chunk_ids: Vec<String> = b["chunk_ids"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            MapBatchState {
                batch_id,
                chunk_ids,
                status: BatchStatus::Pending,
                claimed_by: None,
                claimed_at_unix: None,
                completed_at_unix: None,
                output: None,
            }
        })
        .collect();

    let plan = MapPlan {
        plan_id: plan_id.clone(),
        session_id: session_id.to_string(),
        created_at_unix: super::persistence::unix_now(),
        batch_size,
        batches,
    };
    persist_plan(&plan)?;
    Ok(plan_id)
}

pub fn claim(plan_id: &str, worker_id: &str, batch_id: Option<&str>) -> Result<Value> {
    let _lock = PlanLock::acquire(plan_id)?;
    let mut plan = load_plan(plan_id)?;

    let idx = if let Some(bid) = batch_id {
        plan.batches
            .iter()
            .position(|b| b.batch_id == bid)
            .ok_or_else(|| Error::InvalidArgument(format!("unknown batch_id: {bid}")))?
    } else {
        plan.batches
            .iter()
            .position(|b| b.status == BatchStatus::Pending)
            .unwrap_or(usize::MAX)
    };

    if idx == usize::MAX {
        return Ok(json!({
            "plan_id": plan_id,
            "session_id": plan.session_id,
            "status": "no_work",
            "worker_id": worker_id,
            "remaining_pending": 0,
            "hint": "All batches claimed or completed; merge worker outputs with rlm_reduce_merge"
        }));
    }

    match plan.batches[idx].status {
        BatchStatus::Pending => {
            plan.batches[idx].status = BatchStatus::Claimed;
            plan.batches[idx].claimed_by = Some(worker_id.to_string());
            plan.batches[idx].claimed_at_unix = Some(super::persistence::unix_now());
        }
        BatchStatus::Claimed => {
            if plan.batches[idx].claimed_by.as_deref() != Some(worker_id) {
                return Err(Error::InvalidArgument(format!(
                    "batch {} already claimed by {}",
                    plan.batches[idx].batch_id,
                    plan.batches[idx].claimed_by.as_deref().unwrap_or("unknown")
                )));
            }
        }
        BatchStatus::Completed => {
            return Err(Error::InvalidArgument(format!(
                "batch {} already completed",
                plan.batches[idx].batch_id
            )));
        }
    }

    let batch_id_out = plan.batches[idx].batch_id.clone();
    let chunk_ids = plan.batches[idx].chunk_ids.clone();
    let remaining = plan
        .batches
        .iter()
        .filter(|b| b.status == BatchStatus::Pending)
        .count();
    persist_plan(&plan)?;

    Ok(json!({
        "plan_id": plan_id,
        "session_id": plan.session_id,
        "status": "claimed",
        "batch_id": batch_id_out,
        "chunk_ids": chunk_ids,
        "worker_id": worker_id,
        "remaining_pending": remaining,
        "hint": "Call rlm_chunk with chunk_ids, then rlm_map_complete with worker output JSON"
    }))
}

pub fn complete(plan_id: &str, worker_id: &str, batch_id: &str, output: Value) -> Result<Value> {
    if output.get("findings").and_then(|v| v.as_array()).is_none() {
        return Err(Error::InvalidArgument(
            "worker output must include findings array".into(),
        ));
    }

    let _lock = PlanLock::acquire(plan_id)?;
    let mut plan = load_plan(plan_id)?;

    let batch = plan
        .batches
        .iter_mut()
        .find(|b| b.batch_id == batch_id)
        .ok_or_else(|| Error::InvalidArgument(format!("unknown batch_id: {batch_id}")))?;

    if batch.status == BatchStatus::Completed {
        if batch.claimed_by.as_deref() == Some(worker_id) {
            let completed = plan
                .batches
                .iter()
                .filter(|b| b.status == BatchStatus::Completed)
                .count();
            return Ok(status_response(
                plan_id,
                batch_id,
                completed,
                plan.batches.len(),
            ));
        }
        return Err(Error::InvalidArgument(format!(
            "batch {batch_id} already completed by another worker"
        )));
    }

    if batch.status != BatchStatus::Claimed {
        return Err(Error::InvalidArgument(format!(
            "batch {batch_id} is not claimed"
        )));
    }

    if batch.claimed_by.as_deref() != Some(worker_id) {
        return Err(Error::InvalidArgument(format!(
            "batch {batch_id} claimed by {}, not {worker_id}",
            batch.claimed_by.as_deref().unwrap_or("unknown")
        )));
    }

    batch.status = BatchStatus::Completed;
    batch.completed_at_unix = Some(super::persistence::unix_now());
    batch.output = Some(output);

    let completed = plan
        .batches
        .iter()
        .filter(|b| b.status == BatchStatus::Completed)
        .count();
    let total = plan.batches.len();
    persist_plan(&plan)?;

    Ok(status_response(plan_id, batch_id, completed, total))
}

fn status_response(plan_id: &str, batch_id: &str, completed: usize, total: usize) -> Value {
    let all_complete = completed == total;
    json!({
        "plan_id": plan_id,
        "batch_id": batch_id,
        "status": "completed",
        "completed_batches": completed,
        "total_batches": total,
        "all_complete": all_complete,
        "hint": if all_complete {
            "All batches done; call rlm_reduce_merge with worker outputs"
        } else {
            "Claim next batch with rlm_map_claim or wait for other workers"
        }
    })
}

#[allow(dead_code)]
pub fn completed_outputs(plan_id: &str) -> Result<Vec<Value>> {
    let plan = load_plan(plan_id)?;
    Ok(plan
        .batches
        .iter()
        .filter_map(|b| b.output.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;
    use tempfile::TempDir;

    fn with_cache<F: FnOnce()>(f: F) {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());
        f();
        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn claim_and_complete_two_workers_no_duplicate() {
        with_cache(|| {
            let batches = vec![
                json!({ "batch_id": "batch-0", "chunk_ids": ["c-0"] }),
                json!({ "batch_id": "batch-1", "chunk_ids": ["c-1"] }),
            ];
            let plan_id = create_and_persist("sess-1", 1, &batches).unwrap();

            let w1 = claim(&plan_id, "worker-a", None).unwrap();
            assert_eq!(w1["status"], "claimed");
            assert_eq!(w1["batch_id"], "batch-0");

            let w2 = claim(&plan_id, "worker-b", None).unwrap();
            assert_eq!(w2["status"], "claimed");
            assert_eq!(w2["batch_id"], "batch-1");

            let none = claim(&plan_id, "worker-c", None).unwrap();
            assert_eq!(none["status"], "no_work");

            let out = json!({
                "batch_id": "batch-0",
                "worker_id": "worker-a",
                "findings": [{ "summary": "done" }],
                "unresolved": []
            });
            let done = complete(&plan_id, "worker-a", "batch-0", out).unwrap();
            assert_eq!(done["completed_batches"], 1);
            assert_eq!(done["all_complete"], false);

            let outputs = completed_outputs(&plan_id).unwrap();
            assert_eq!(outputs.len(), 1);
        });
    }

    #[test]
    fn double_claim_same_batch_rejected() {
        with_cache(|| {
            let batches = vec![json!({ "batch_id": "batch-0", "chunk_ids": ["c-0"] })];
            let plan_id = create_and_persist("sess-1", 1, &batches).unwrap();
            claim(&plan_id, "worker-a", None).unwrap();
            let err = claim(&plan_id, "worker-b", Some("batch-0")).unwrap_err();
            assert!(err.to_string().contains("already claimed"));
        });
    }
}
