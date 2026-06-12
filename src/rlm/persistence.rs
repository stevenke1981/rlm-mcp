use crate::error::{Error, Result};
use crate::rlm::session::ScanSession;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[cfg(test)]
pub const SESSION_TTL_SECS: u64 = 3600;

const LOCK_RETRIES: u32 = 8;
const LOCK_SLEEP_MS: u64 = 25;

pub fn sessions_dir() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-sessions")
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn session_file_path(id: &str) -> PathBuf {
    sessions_dir().join(format!("{id}.json"))
}

pub fn deleted_marker_path(id: &str) -> PathBuf {
    sessions_dir().join(format!("{id}.deleted"))
}

pub fn is_session_deleted(id: &str) -> bool {
    deleted_marker_path(id).exists()
}

fn is_session_artifact(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.ends_with(".tmp") || name.ends_with(".lock") || name.ends_with(".deleted") {
        return false;
    }
    path.extension().and_then(|e| e.to_str()) == Some("json")
}

fn session_id_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

struct SessionLock {
    path: PathBuf,
}

impl SessionLock {
    fn acquire(id: &str) -> Result<Self> {
        let dir = sessions_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{id}.lock"));
        for attempt in 0..LOCK_RETRIES {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "pid={} ts={}", std::process::id(), unix_now());
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt + 1 == LOCK_RETRIES {
                        return Err(Error::Other(format!(
                            "session lock busy: {id} (another writer active)"
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(LOCK_SLEEP_MS));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Other(format!("session lock busy: {id}")))
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn atomic_write_json(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("session path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Other("invalid session path".into()))?;
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

pub fn persist_session(session: &ScanSession) -> Result<()> {
    if is_session_deleted(&session.id) {
        return Err(Error::SessionNotFound(session.id.clone()));
    }
    let _lock = SessionLock::acquire(&session.id)?;
    let path = session_file_path(&session.id);
    let content = serde_json::to_string(session)?;
    atomic_write_json(&path, &content)
}

pub fn remove_session_file(id: &str) -> Result<()> {
    let _lock = SessionLock::acquire(id)?;
    let marker = deleted_marker_path(id);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)?;
    let result = (|| {
        let path = session_file_path(id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(marker);
    result
}

pub fn load_session_by_id(id: &str) -> Result<Option<ScanSession>> {
    if is_session_deleted(id) {
        return Ok(None);
    }
    let path = session_file_path(id);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    match serde_json::from_str::<ScanSession>(&content) {
        Ok(mut session) => {
            super::session::normalize_session_public(&mut session);
            Ok(Some(session))
        }
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            Ok(None)
        }
    }
}

pub fn load_persisted_sessions() -> Result<Vec<ScanSession>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !is_session_artifact(&path) {
            continue;
        }
        let Some(id) = session_id_from_path(&path) else {
            continue;
        };
        if is_session_deleted(&id) {
            continue;
        }
        if let Ok(Some(session)) = load_session_by_id(&id) {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

pub fn list_disk_session_ids() -> Result<Vec<String>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !is_session_artifact(&path) {
            continue;
        }
        if let Some(id) = session_id_from_path(&path) {
            if !is_session_deleted(&id) {
                ids.push(id);
            }
        }
    }
    ids.sort();
    Ok(ids)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CleanupReport {
    pub removed_count: usize,
    pub removed_ids: Vec<String>,
}

pub fn cleanup_expired_on_disk(ttl_secs: u64, max_sessions: usize) -> Result<CleanupReport> {
    let mut sessions: std::collections::HashMap<String, ScanSession> =
        std::collections::HashMap::new();
    for session in load_persisted_sessions()? {
        sessions.insert(session.id.clone(), session);
    }
    let before: std::collections::HashSet<_> = sessions.keys().cloned().collect();
    purge_expired(&mut sessions, ttl_secs)?;
    trim_to_limit(&mut sessions, max_sessions)?;
    let after: std::collections::HashSet<_> = sessions.keys().cloned().collect();
    let removed_ids: Vec<_> = before.difference(&after).cloned().collect();
    Ok(CleanupReport {
        removed_count: removed_ids.len(),
        removed_ids,
    })
}

pub fn purge_expired(
    sessions: &mut std::collections::HashMap<String, ScanSession>,
    ttl_secs: u64,
) -> Result<()> {
    let now = unix_now();
    let expired: Vec<String> = sessions
        .iter()
        .filter(|(_, s)| {
            if s.expires_at_unix > 0 {
                now >= s.expires_at_unix
            } else {
                now.saturating_sub(s.created_at_unix) > ttl_secs
            }
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        sessions.remove(&id);
        let _ = remove_session_file(&id);
    }
    Ok(())
}

pub fn trim_to_limit(
    sessions: &mut std::collections::HashMap<String, ScanSession>,
    max_sessions: usize,
) -> Result<()> {
    if sessions.len() <= max_sessions {
        return Ok(());
    }
    let mut ids: Vec<_> = sessions
        .values()
        .map(|s| (s.id.clone(), s.created_at_unix))
        .collect();
    ids.sort_by_key(|(_, created)| *created);
    let remove_count = sessions.len() - max_sessions;
    for (id, _) in ids.into_iter().take(remove_count) {
        sessions.remove(&id);
        let _ = remove_session_file(&id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rlm::session::{Chunk, ScanSession};
    use crate::test_lock;
    use tempfile::TempDir;

    fn sample_session(id: &str) -> ScanSession {
        ScanSession {
            id: id.into(),
            root_path: "/tmp".into(),
            source_kind: "path".into(),
            chunks: vec![Chunk {
                id: "c-0".into(),
                path: "a.txt".into(),
                offset: 0,
                line_count: 1,
                content: "test".into(),
            }],
            total_bytes: 4,
            files_scanned: 1,
            files_skipped: 0,
            skip_reasons: std::collections::HashMap::new(),
            variables: std::collections::HashMap::new(),
            created_at_unix: unix_now(),
            expires_at_unix: unix_now().saturating_add(SESSION_TTL_SECS),
            revision: 1,
        }
    }

    #[test]
    fn atomic_persist_survives_read() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let session = sample_session("atomic-test");
        persist_session(&session).unwrap();
        let loaded = load_persisted_sessions().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "atomic-test");

        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn delete_marker_blocks_reload() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let session = sample_session("gone");
        persist_session(&session).unwrap();
        remove_session_file("gone").unwrap();
        assert!(load_session_by_id("gone").unwrap().is_none());

        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn cleanup_removes_expired_sessions() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let mut session = sample_session("old");
        session.expires_at_unix = unix_now().saturating_sub(10);
        persist_session(&session).unwrap();
        let report = cleanup_expired_on_disk(SESSION_TTL_SECS, 50).unwrap();
        assert!(report.removed_count >= 1);
        assert!(load_session_by_id("old").unwrap().is_none());

        std::env::remove_var("RLM_CACHE_DIR");
    }
}
