mod persistence;
mod session;
mod workflow;

pub use session::*;
pub use workflow::*;

use crate::cbm_client::CbmClient;
use crate::error::Result;
use crate::project::normalize_project_name;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// RLM orchestrator: graph tools via CBM MCP + local scan sessions.
pub struct RlmEngine {
    sessions: Arc<Mutex<SessionStore>>,
    cbm: Arc<CbmClient>,
}

impl RlmEngine {
    pub fn new(cbm: Arc<CbmClient>) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(SessionStore::new())),
            cbm,
        }
    }

    pub fn workflow(&self, phase: &str) -> Value {
        workflow_guidance(phase)
    }

    pub fn index_status(&self, project: &str) -> Result<Value> {
        let project = normalize_project_name(project);
        self.cbm.index_status(&project)
    }

    pub fn filter(
        &self,
        project: &str,
        query: Option<&str>,
        pattern: Option<&str>,
        label: Option<&str>,
        limit: u64,
    ) -> Result<Value> {
        let project = normalize_project_name(project);
        if query.is_some() || label.is_some() {
            return self
                .cbm
                .search_graph(&project, query, None, label, limit);
        }
        if let Some(pat) = pattern {
            return self.cbm.search_code_files(&project, pat, None, limit);
        }
        Err(crate::error::Error::InvalidArgument(
            "Provide query/label (graph search) or pattern (file path search)".into(),
        ))
    }

    pub fn read_symbol(&self, project: &str, qualified_name: &str) -> Result<Value> {
        let project = normalize_project_name(project);
        self.cbm.get_code_snippet(&project, qualified_name)
    }

    pub fn trace(
        &self,
        project: &str,
        function_name: &str,
        direction: &str,
        depth: u64,
        mode: &str,
    ) -> Result<Value> {
        let project = normalize_project_name(project);
        self.cbm
            .trace_path(&project, function_name, direction, depth, mode)
    }

    pub fn architecture(&self, project: &str) -> Result<Value> {
        let project = normalize_project_name(project);
        self.cbm.get_architecture(&project)
    }

    pub fn detect_changes(&self, project: &str, scope: Option<&str>) -> Result<Value> {
        let project = normalize_project_name(project);
        self.cbm.detect_changes(&project, scope)
    }

    pub fn scan(&self, path: &str) -> Result<Value> {
        let session = self.sessions.lock().unwrap().create_from_path(path)?;
        Ok(json!({
            "session_id": session.id,
            "file_count": session.files_scanned,
            "chunk_count": session.chunks.len(),
            "total_bytes": session.total_bytes,
            "files_scanned": session.files_scanned,
            "files_skipped": session.files_skipped,
            "hint": "Use rlm_chunk or rlm_peek to read chunks"
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