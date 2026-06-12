use crate::discover::{
    configure_walker, language_for_path, IndexMode, SKIP_FILENAMES, SKIP_SUFFIXES,
};
use crate::error::{Error, Result};
use crate::rlm::config::RlmConfig;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Chunk {
    #[serde(default)]
    pub id: String,
    pub path: String,
    pub offset: usize,
    pub line_count: usize,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanSession {
    pub id: String,
    pub root_path: String,
    #[serde(default = "default_source_kind")]
    pub source_kind: String,
    pub chunks: Vec<Chunk>,
    pub total_bytes: usize,
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub skip_reasons: HashMap<String, usize>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    #[serde(default = "default_created_at")]
    pub created_at_unix: u64,
    #[serde(default)]
    pub expires_at_unix: u64,
    #[serde(default)]
    pub revision: u64,
}

fn default_source_kind() -> String {
    "path".into()
}

fn default_created_at() -> u64 {
    super::persistence::unix_now()
}

pub struct SessionStore {
    sessions: HashMap<String, ScanSession>,
    config: RlmConfig,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::with_config(RlmConfig::default())
    }

    pub fn with_config(config: RlmConfig) -> Self {
        let mut sessions = HashMap::new();
        if let Ok(loaded) = super::persistence::load_persisted_sessions() {
            for mut session in loaded {
                normalize_session(&mut session);
                sessions.insert(session.id.clone(), session);
            }
        }
        let mut store = Self { sessions, config };
        let _ = store.purge_expired();
        store
    }

    fn purge_expired(&mut self) -> Result<()> {
        super::persistence::purge_expired(&mut self.sessions, self.config.session_ttl_secs)?;
        super::persistence::trim_to_limit(&mut self.sessions, self.config.max_sessions)?;
        Ok(())
    }

    pub fn create_from_path(&mut self, path: &str) -> Result<ScanSession> {
        let root = super::safety::resolve_scan_path(path)?;

        let mut chunks = Vec::new();
        let mut total_bytes = 0usize;
        let mut files_scanned = 0usize;
        let mut files_skipped = 0usize;
        let mut skip_reasons: HashMap<String, usize> = HashMap::new();

        let walker = if root.is_file() {
            vec![root.clone()]
        } else {
            configure_walker(&root, IndexMode::Full)
                .build()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                .map(|e| e.path().to_path_buf())
                .collect()
        };

        for file_path in walker {
            if total_bytes >= self.config.max_total_bytes {
                *skip_reasons.entry("budget_exceeded".into()).or_default() += 1;
                files_skipped += 1;
                continue;
            }
            if chunks.len() >= self.config.max_chunks {
                *skip_reasons.entry("chunk_limit".into()).or_default() += 1;
                files_skipped += 1;
                continue;
            }

            if let Some(name) = file_path.file_name().and_then(|n| n.to_str()) {
                if SKIP_FILENAMES.contains(&name) {
                    *skip_reasons.entry("skip_filename".into()).or_default() += 1;
                    files_skipped += 1;
                    continue;
                }
            }

            if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
                let dotted = format!(".{ext}");
                if SKIP_SUFFIXES.contains(&dotted.as_str()) {
                    *skip_reasons.entry("binary_or_asset".into()).or_default() += 1;
                    files_skipped += 1;
                    continue;
                }
            }

            let meta = match file_path.metadata() {
                Ok(m) => m,
                Err(_) => {
                    *skip_reasons.entry("unreadable".into()).or_default() += 1;
                    files_skipped += 1;
                    continue;
                }
            };
            if meta.len() > self.config.max_file_bytes {
                *skip_reasons.entry("file_too_large".into()).or_default() += 1;
                files_skipped += 1;
                continue;
            }

            let bytes = match std::fs::read(&file_path) {
                Ok(b) => b,
                Err(_) => {
                    *skip_reasons.entry("unreadable".into()).or_default() += 1;
                    files_skipped += 1;
                    continue;
                }
            };
            if super::safety::is_probably_binary(&bytes) {
                *skip_reasons
                    .entry("binary_or_unreadable".into())
                    .or_default() += 1;
                files_skipped += 1;
                continue;
            }
            let content = match String::from_utf8(bytes) {
                Ok(c) => c,
                Err(_) => {
                    *skip_reasons
                        .entry("binary_or_unreadable".into())
                        .or_default() += 1;
                    files_skipped += 1;
                    continue;
                }
            };

            if language_for_path(&file_path).is_none() && content.len() > 64 * 1024 {
                *skip_reasons.entry("non_code_large".into()).or_default() += 1;
                files_skipped += 1;
                continue;
            }

            total_bytes += content.len();
            files_scanned += 1;

            let rel = file_path
                .strip_prefix(if root.is_file() {
                    root.parent().unwrap_or(&root)
                } else {
                    &root
                })
                .unwrap_or(&file_path)
                .to_string_lossy()
                .replace('\\', "/");
            append_chunks(
                &mut chunks,
                &rel,
                &content,
                self.config.chunk_lines,
                self.config.max_chunks,
            );
        }

        self.finalize_session(
            root.to_string_lossy().to_string(),
            "path",
            chunks,
            total_bytes,
            files_scanned,
            files_skipped,
            skip_reasons,
            HashMap::new(),
        )
    }

    pub fn create_from_text(
        &mut self,
        content: &str,
        virtual_path: &str,
        variables: HashMap<String, String>,
    ) -> Result<ScanSession> {
        if super::safety::is_probably_binary(content.as_bytes()) {
            return Err(Error::InvalidArgument(
                "binary content not supported".into(),
            ));
        }
        if content.len() > self.config.max_total_bytes {
            return Err(Error::InvalidArgument(format!(
                "content exceeds max total bytes ({})",
                self.config.max_total_bytes
            )));
        }

        let mut chunks = Vec::new();
        append_chunks(
            &mut chunks,
            virtual_path,
            content,
            self.config.chunk_lines,
            self.config.max_chunks,
        );

        self.finalize_session(
            format!("text://{virtual_path}"),
            "text",
            chunks,
            content.len(),
            1,
            0,
            HashMap::new(),
            variables,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn finalize_session(
        &mut self,
        root_path: String,
        source_kind: &str,
        chunks: Vec<Chunk>,
        total_bytes: usize,
        files_scanned: usize,
        files_skipped: usize,
        skip_reasons: HashMap<String, usize>,
        variables: HashMap<String, String>,
    ) -> Result<ScanSession> {
        let now = super::persistence::unix_now();
        let mut session = ScanSession {
            id: Uuid::new_v4().to_string(),
            root_path,
            source_kind: source_kind.into(),
            chunks,
            total_bytes,
            files_scanned,
            files_skipped,
            skip_reasons,
            variables,
            created_at_unix: now,
            expires_at_unix: now.saturating_add(self.config.session_ttl_secs),
            revision: 1,
        };
        assign_chunk_ids(&mut session);
        self.sessions.insert(session.id.clone(), session.clone());
        super::persistence::persist_session(&session)?;
        let _ = self.purge_expired();
        Ok(session)
    }

    pub fn get(&self, id: &str) -> Result<&ScanSession> {
        if super::persistence::is_session_deleted(id) {
            return Err(Error::SessionNotFound(id.to_string()));
        }
        self.sessions
            .get(id)
            .ok_or_else(|| Error::SessionNotFound(id.to_string()))
    }

    pub fn hydrate(&mut self, id: &str) -> Result<()> {
        if self.sessions.contains_key(id) {
            return Ok(());
        }
        if let Some(session) = super::persistence::load_session_by_id(id)? {
            self.sessions.insert(id.to_string(), session);
            return Ok(());
        }
        Err(Error::SessionNotFound(id.to_string()))
    }

    pub fn get_or_hydrate(&mut self, id: &str) -> Result<&ScanSession> {
        let _ = self.hydrate(id);
        self.get(id)
    }

    pub fn get_chunk(&mut self, session_id: &str, chunk_id: &str) -> Result<&Chunk> {
        let session = self.get_or_hydrate(session_id)?;
        session
            .chunks
            .iter()
            .find(|c| c.id == chunk_id)
            .ok_or_else(|| Error::InvalidArgument(format!("chunk not found: {chunk_id}")))
    }

    pub fn list(&self) -> Vec<serde_json::Value> {
        let mut merged: std::collections::HashMap<String, ScanSession> =
            std::collections::HashMap::new();
        for session in self.sessions.values() {
            merged.insert(session.id.clone(), session.clone());
        }
        if let Ok(ids) = super::persistence::list_disk_session_ids() {
            for id in ids {
                if merged.contains_key(&id) {
                    continue;
                }
                if let Ok(Some(session)) = super::persistence::load_session_by_id(&id) {
                    merged.insert(id, session);
                }
            }
        }
        let mut out: Vec<_> = merged.values().map(session_summary).collect();
        out.sort_by(|a, b| {
            a["id"]
                .as_str()
                .unwrap_or("")
                .cmp(b["id"].as_str().unwrap_or(""))
        });
        out
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let existed = self.sessions.remove(id).is_some()
            || super::persistence::session_file_path(id).exists();
        if !existed {
            return Err(Error::SessionNotFound(id.to_string()));
        }
        super::persistence::remove_session_file(id)?;
        Ok(())
    }

    pub fn cleanup_expired(&mut self) -> Result<super::persistence::CleanupReport> {
        let _ = self.purge_expired();
        let report = super::persistence::cleanup_expired_on_disk(
            self.config.session_ttl_secs,
            self.config.max_sessions,
        )?;
        for id in &report.removed_ids {
            self.sessions.remove(id);
        }
        Ok(report)
    }

    pub fn export(&mut self, id: &str) -> Result<ScanSession> {
        self.get_or_hydrate(id)?;
        Ok(self.sessions.get(id).unwrap().clone())
    }

    pub fn import_session(
        &mut self,
        mut session: ScanSession,
        preserve_id: bool,
    ) -> Result<ScanSession> {
        if !preserve_id || session.id.is_empty() {
            session.id = Uuid::new_v4().to_string();
        }
        if super::persistence::is_session_deleted(&session.id) {
            return Err(Error::InvalidArgument(format!(
                "cannot import deleted session id: {}",
                session.id
            )));
        }
        session.revision = session.revision.max(1);
        assign_chunk_ids(&mut session);
        if session.expires_at_unix == 0 {
            session.expires_at_unix =
                super::persistence::unix_now().saturating_add(self.config.session_ttl_secs);
        }
        self.sessions.insert(session.id.clone(), session.clone());
        super::persistence::persist_session(&session)?;
        let _ = self.purge_expired();
        Ok(session)
    }
}

fn session_summary(s: &ScanSession) -> serde_json::Value {
    serde_json::json!({
        "id": s.id,
        "root_path": s.root_path,
        "source_kind": s.source_kind,
        "chunk_count": s.chunks.len(),
        "total_bytes": s.total_bytes,
        "files_scanned": s.files_scanned,
        "files_skipped": s.files_skipped,
        "created_at_unix": s.created_at_unix,
        "expires_at_unix": s.expires_at_unix,
        "revision": s.revision,
    })
}

pub(crate) fn normalize_session_public(session: &mut ScanSession) {
    normalize_session(session);
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn append_chunks(
    chunks: &mut Vec<Chunk>,
    path: &str,
    content: &str,
    chunk_lines: usize,
    max_chunks: usize,
) {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return;
    }
    for (i, window) in lines.chunks(chunk_lines.max(1)).enumerate() {
        if chunks.len() >= max_chunks {
            break;
        }
        chunks.push(Chunk {
            id: String::new(),
            path: path.to_string(),
            offset: i * chunk_lines,
            line_count: window.len(),
            content: window.join("\n"),
        });
    }
}

fn assign_chunk_ids(session: &mut ScanSession) {
    for (i, chunk) in session.chunks.iter_mut().enumerate() {
        if chunk.id.is_empty() {
            chunk.id = format!("c-{i}");
        }
    }
}

fn normalize_session(session: &mut ScanSession) {
    if session.expires_at_unix == 0 {
        session.expires_at_unix = session
            .created_at_unix
            .saturating_add(RlmConfig::default().session_ttl_secs);
    }
    assign_chunk_ids(session);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;
    use tempfile::TempDir;

    #[test]
    fn persisted_session_survives_store_reopen() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.txt"), "hello\nworld\n").unwrap();

        let mut store = SessionStore::new();
        let session = store
            .create_from_path(dir.path().to_string_lossy().as_ref())
            .unwrap();
        let id = session.id.clone();
        assert!(!session.chunks[0].id.is_empty());
        drop(store);

        let store2 = SessionStore::new();
        let loaded = store2
            .get(&id)
            .expect("session should persist across invocations");
        assert!(!loaded.chunks.is_empty());
        assert_eq!(loaded.chunks[0].id, "c-0");

        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn create_from_text_assigns_chunk_ids() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let mut store = SessionStore::new();
        let session = store
            .create_from_text("line one\nline two\n", "prompt.txt", HashMap::new())
            .unwrap();
        assert_eq!(session.source_kind, "text");
        assert!(!session.chunks.is_empty());
        assert_eq!(session.chunks[0].id, "c-0");

        std::env::remove_var("RLM_CACHE_DIR");
    }
}
