use serde_json::{json, Value};

pub fn reduce_schema() -> Value {
    json!({
        "phase": "reduce",
        "description": "Merge map-phase worker outputs into a final structured answer",
        "worker_schema": super::map::worker_output_schema(),
        "final_answer_schema": {
            "type": "object",
            "required": ["answer", "evidence"],
            "properties": {
                "answer": { "type": "string" },
                "evidence": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "chunk_id": { "type": "string" },
                            "path": { "type": "string" },
                            "line": { "type": "integer" },
                            "snippet": { "type": "string" }
                        }
                    }
                },
                "coverage": {
                    "type": "object",
                    "properties": {
                        "chunks_visited": { "type": "integer" },
                        "chunks_total": { "type": "integer" },
                        "gaps": { "type": "array", "items": { "type": "string" } }
                    }
                },
                "unresolved_questions": { "type": "array", "items": { "type": "string" } },
                "needs_recursion": { "type": "boolean" },
                "recursion_hint": { "type": "string" }
            }
        },
        "checklist": [
            "Every finding cites chunk_id or path evidence",
            "Unresolved questions listed explicitly",
            "Set needs_recursion=true only when gaps require another filter→map pass",
            "Do not re-read entire session at reduce time"
        ]
    })
}

pub fn reduce_merge(worker_outputs: &[Value]) -> Value {
    let mut all_findings = Vec::new();
    let mut all_unresolved = Vec::new();
    let mut workers = Vec::new();
    let mut chunk_ids = std::collections::HashSet::new();
    let mut paths = std::collections::HashSet::new();

    for output in worker_outputs {
        let batch_id = output
            .get("batch_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let worker_id = output
            .get("worker_id")
            .and_then(|v| v.as_str())
            .unwrap_or(batch_id);
        workers.push(json!({ "batch_id": batch_id, "worker_id": worker_id }));

        if let Some(findings) = output.get("findings").and_then(|v| v.as_array()) {
            for finding in findings {
                if let Some(ids) = finding.get("chunk_ids").and_then(|v| v.as_array()) {
                    for id in ids {
                        if let Some(s) = id.as_str() {
                            chunk_ids.insert(s.to_string());
                        }
                    }
                }
                if let Some(ps) = finding.get("paths").and_then(|v| v.as_array()) {
                    for p in ps {
                        if let Some(s) = p.as_str() {
                            paths.insert(s.to_string());
                        }
                    }
                }
                all_findings.push(finding.clone());
            }
        }

        if let Some(unresolved) = output.get("unresolved").and_then(|v| v.as_array()) {
            for q in unresolved {
                all_unresolved.push(q.clone());
            }
        }
    }

    let needs_recursion = !all_unresolved.is_empty();

    json!({
        "workers": workers,
        "finding_count": all_findings.len(),
        "findings": all_findings,
        "chunk_ids_cited": chunk_ids.into_iter().collect::<Vec<_>>(),
        "paths_cited": paths.into_iter().collect::<Vec<_>>(),
        "unresolved_questions": all_unresolved,
        "needs_recursion": needs_recursion,
        "recursion_hint": if needs_recursion {
            "Run rlm_peek on unresolved areas, then rlm_map_plan for gaps only"
        } else {
            "Sufficient coverage — produce final answer from merged findings"
        },
        "next_step": if needs_recursion { "filter_map_recursion" } else { "final_answer" }
    })
}
