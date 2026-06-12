use crate::rlm::session::ScanSession;
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn env_info(session: &ScanSession) -> Value {
    let mut files: HashMap<String, Value> = HashMap::new();

    for chunk in &session.chunks {
        let entry = files.entry(chunk.path.clone()).or_insert_with(|| {
            json!({
                "path": chunk.path,
                "chunk_count": 0,
                "line_count": 0,
                "bytes": 0,
                "chunk_ids": []
            })
        });
        entry["chunk_count"] = json!(entry["chunk_count"].as_u64().unwrap_or(0) + 1);
        entry["line_count"] =
            json!(entry["line_count"].as_u64().unwrap_or(0) + chunk.line_count as u64);
        entry["bytes"] = json!(entry["bytes"].as_u64().unwrap_or(0) + chunk.content.len() as u64);
        if let Some(ids) = entry["chunk_ids"].as_array_mut() {
            ids.push(json!(chunk.id));
        }
    }

    let mut file_list: Vec<_> = files.into_values().collect();
    file_list.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or("")
            .cmp(b["path"].as_str().unwrap_or(""))
    });

    json!({
        "session_id": session.id,
        "root_path": session.root_path,
        "source_kind": session.source_kind,
        "context_length_bytes": session.total_bytes,
        "chunk_count": session.chunks.len(),
        "file_count": session.files_scanned,
        "files_skipped": session.files_skipped,
        "skip_reasons": session.skip_reasons,
        "variables": session.variables,
        "created_at_unix": session.created_at_unix,
        "expires_at_unix": session.expires_at_unix,
        "files": file_list,
        "hint": "Use rlm_peek to filter, rlm_chunk to read, rlm_slice for lines, rlm_transform for safe ops, rlm_artifact_* for derived state"
    })
}

pub fn slice_chunk(
    chunk: &crate::rlm::session::Chunk,
    start_line: usize,
    end_line: usize,
) -> Value {
    let lines: Vec<&str> = chunk.content.lines().collect();
    let start_idx = start_line.saturating_sub(chunk.offset + 1);
    let end_idx = end_line.saturating_sub(chunk.offset).min(lines.len());
    let slice = if start_idx < lines.len() && start_idx < end_idx {
        lines[start_idx..end_idx].join("\n")
    } else {
        String::new()
    };

    json!({
        "chunk_id": chunk.id,
        "path": chunk.path,
        "start_line": start_line,
        "end_line": end_line,
        "line_count": end_idx.saturating_sub(start_idx),
        "content": slice
    })
}
