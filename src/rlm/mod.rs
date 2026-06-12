mod persistence;
mod session;
mod workflow;

pub use session::*;
pub use workflow::*;

use crate::error::Result;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// RLM orchestrator: external context via scan sessions (filter → map → reduce).
pub struct RlmEngine {
    sessions: Arc<Mutex<SessionStore>>,
}

impl RlmEngine {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(SessionStore::new())),
        }
    }

    pub fn workflow(&self, phase: &str) -> Value {
        workflow_guidance(phase)
    }

    pub fn scan(&self, path: &str) -> Result<Value> {
        let session = self.sessions.lock().unwrap().create_from_path(path)?;
        Ok(json!({
            "session_id": session.id,
            "root_path": session.root_path,
            "file_count": session.files_scanned,
            "chunk_count": session.chunks.len(),
            "total_bytes": session.total_bytes,
            "files_scanned": session.files_scanned,
            "files_skipped": session.files_skipped,
            "skip_reasons": session.skip_reasons,
            "hint": "Use rlm_peek to filter, rlm_chunk to read paginated chunks"
        }))
    }

    pub fn chunk(
        &self,
        session_id: &str,
        file_pattern: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let filtered: Vec<_> = session
            .chunks
            .iter()
            .filter(|c| {
                file_pattern.map_or(true, |pat| {
                    c.path.contains(pat)
                        || c.path.ends_with(pat)
                        || glob_match(pat, &c.path)
                })
            })
            .cloned()
            .collect();
        let chunks: Vec<_> = filtered.iter().skip(offset).take(limit).cloned().collect();
        Ok(json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "total": filtered.len(),
            "chunks": chunks
        }))
    }

    pub fn peek(&self, session_id: &str, query: &str, limit: usize) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let matches: Vec<_> = session
            .chunks
            .iter()
            .filter(|c| c.content.contains(query) || c.path.contains(query))
            .take(limit)
            .cloned()
            .collect();
        Ok(json!({
            "session_id": session_id,
            "query": query,
            "match_count": matches.len(),
            "matches": matches
        }))
    }

    pub fn session_list(&self) -> Value {
        let store = self.sessions.lock().unwrap();
        json!({ "sessions": store.list() })
    }

    pub fn session_delete(&self, session_id: &str) -> Result<Value> {
        self.sessions.lock().unwrap().delete(session_id)?;
        Ok(json!({ "session_id": session_id, "deleted": true }))
    }
}

impl Default for RlmEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    simple_glob(pattern, file_name) || simple_glob(pattern, path)
}

fn simple_glob(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text.contains(pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut start = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text.starts_with(part) {
                return false;
            }
            start = part.len();
        } else if i == parts.len() - 1 {
            if !text[start..].ends_with(part) {
                return false;
            }
        } else if let Some(pos) = text[start..].find(part) {
            start += pos + part.len();
        } else {
            return false;
        }
    }
    true
}