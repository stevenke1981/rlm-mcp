use crate::discover::{
    configure_walker, language_for_path, IndexMode, SKIP_FILENAMES, SKIP_SUFFIXES,
};
use crate::error::{Error, Result};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

const CHUNK_LINES: usize = 200;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Chunk {
    pub path: String,
    pub offset: usize,
    pub line_count: usize,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanSession {
    pub id: String,
    pub root_path: String,
    pub chunks: Vec<Chunk>,
    pub total_bytes: usize,
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub skip_reasons: HashMap<String, usize>,
    #[serde(default = "default_created_at")]
    pub created_at_unix: u64,
}

fn default_created_at() -> u64 {
    super::persistence::unix_now()
}

pub struct SessionStore {
    sessions: HashMap<String, ScanSession>,
}

impl SessionStore {
    pub fn new() -> Self {
        let mut sessions = HashMap::new();
        if let Ok(loaded) = super::persistence::load_persisted_sessions() {
            for session in loaded {
                sessions.insert(session.id.clone(), session);
            }
        }
        let mut store = Self { sessions };
        let _ = store.purge_expired();
        store
    }

    fn purge_expired(&mut self) -> Result<()> {
        super::persistence::purge_expired(&mut self.sessions)?;
        super::persistence::trim_to_limit(&mut self.sessions)?;
        Ok(())
    }

    pub fn create_from_path(&mut self, path: &str) -> Result<ScanSession> {
        let root = Path::new(path)
            .canonicalize()
            .map_err(|e| Error::Other(e.to_string()))?;
        if !root.exists() {
            return Err(Error::InvalidArgument(format!("path not found: {path}")));
        }

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
            if total_bytes >= MAX_TOTAL_BYTES {
                *skip_reasons.entry("budget_exceeded".into()).or_default() += 1;
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
            if meta.len() > MAX_FILE_BYTES {
                *skip_reasons.entry("file_too_large".into()).or_default() += 1;
                files_skipped += 1;
                continue;
            }

            let content = match std::fs::read_to_string(&file_path) {
                Ok(c) if !c.contains('\0') => c,
                _ => {
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
            let lines: Vec<&str> = content.lines().collect();
            if lines.is_empty() {
                continue;
            }
            for (i, window) in lines.chunks(CHUNK_LINES).enumerate() {
                chunks.push(Chunk {
                    path: rel.clone(),
                    offset: i * CHUNK_LINES,
                    line_count: window.len(),
                    content: window.join("\n"),
                });
            }
        }

        let session = ScanSession {
            id: Uuid::new_v4().to_string(),
            root_path: root.to_string_lossy().to_string(),
            chunks,
            total_bytes,
            files_scanned,
            files_skipped,
            skip_reasons,
            created_at_unix: super::persistence::unix_now(),
        };
        self.sessions.insert(session.id.clone(), session.clone());
        super::persistence::persist_session(&session)?;
        let _ = self.purge_expired();
        Ok(session)
    }

    pub fn get(&self, id: &str) -> Result<&ScanSession> {
        self.sessions
            .get(id)
            .ok_or_else(|| Error::SessionNotFound(id.to_string()))
    }

    pub fn list(&self) -> Vec<serde_json::Value> {
        self.sessions
            .values()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "root_path": s.root_path,
                    "chunk_count": s.chunks.len(),
                    "total_bytes": s.total_bytes,
                    "files_scanned": s.files_scanned,
                    "files_skipped": s.files_skipped,
                })
            })
            .collect()
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.sessions.remove(id).is_none() {
            return Err(Error::SessionNotFound(id.to_string()));
        }
        super::persistence::remove_session_file(id)?;
        Ok(())
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
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
        drop(store);

        let store2 = SessionStore::new();
        let loaded = store2
            .get(&id)
            .expect("session should persist across invocations");
        assert!(!loaded.chunks.is_empty());

        std::env::remove_var("RLM_CACHE_DIR");
    }
}