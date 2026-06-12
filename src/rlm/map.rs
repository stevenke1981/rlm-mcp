use crate::rlm::session::ScanSession;
use serde_json::{json, Value};

pub fn map_plan(
    session: &ScanSession,
    chunk_ids: Option<&[String]>,
    file_pattern: Option<&str>,
    batch_size: usize,
) -> Value {
    let selected: Vec<_> = session
        .chunks
        .iter()
        .filter(|c| {
            if let Some(ids) = chunk_ids {
                if !ids.contains(&c.id) {
                    return false;
                }
            }
            if let Some(pat) = file_pattern {
                if !super::glob_match(pat, &c.path) && !c.path.contains(pat) {
                    return false;
                }
            }
            true
        })
        .map(|c| c.id.clone())
        .collect();

    let batches: Vec<_> = selected
        .chunks(batch_size.max(1))
        .enumerate()
        .map(|(i, batch)| {
            json!({
                "batch_id": format!("batch-{i}"),
                "chunk_ids": batch,
                "count": batch.len()
            })
        })
        .collect();

    json!({
        "session_id": session.id,
        "total_chunks": selected.len(),
        "batch_size": batch_size.max(1),
        "batches": batches,
        "worker_output_schema": worker_output_schema(),
        "hint": "Use plan_id with rlm_map_claim for coordinated workers, or assign batch_id manually"
    })
}

pub fn worker_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["batch_id", "findings"],
        "properties": {
            "batch_id": { "type": "string" },
            "worker_id": { "type": "string" },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["summary"],
                    "properties": {
                        "summary": { "type": "string" },
                        "chunk_ids": { "type": "array", "items": { "type": "string" } },
                        "paths": { "type": "array", "items": { "type": "string" } },
                        "confidence": { "type": "number" },
                        "evidence_lines": { "type": "array", "items": { "type": "integer" } }
                    }
                }
            },
            "unresolved": { "type": "array", "items": { "type": "string" } }
        }
    })
}
