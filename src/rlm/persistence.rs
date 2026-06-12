use crate::error::Result;
use crate::rlm::session::ScanSession;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const SESSION_TTL_SECS: u64 = 3600;
pub const MAX_PERSISTED_SESSIONS: usize = 50;

pub fn sessions_dir() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-sessions")
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn persist_session(session: &ScanSession) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", session.id));
    let tmp = dir.join(format!("{}.{}.tmp", session.id, Uuid::new_v4()));
    let content = serde_json::to_string(session)?;
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn remove_session_file(id: &str) -> Result<()> {
    let path = sessions_dir().join(format!("{id}.json"));
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
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
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        match serde_json::from_str::<ScanSession>(&content) {
            Ok(session) => sessions.push(session),
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(sessions)
}

pub fn purge_expired(sessions: &mut std::collections::HashMap<String, ScanSession>) -> Result<()> {
    let now = unix_now();
    let expired: Vec<String> = sessions
        .iter()
        .filter(|(_, s)| now.saturating_sub(s.created_at_unix) > SESSION_TTL_SECS)
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        sessions.remove(&id);
        let _ = remove_session_file(&id);
    }
    Ok(())
}

pub fn trim_to_limit(sessions: &mut std::collections::HashMap<String, ScanSession>) -> Result<()> {
    if sessions.len() <= MAX_PERSISTED_SESSIONS {
        return Ok(());
    }
    let mut ids: Vec<_> = sessions
        .values()
        .map(|s| (s.id.clone(), s.created_at_unix))
        .collect();
    ids.sort_by_key(|(_, created)| *created);
    let remove_count = sessions.len() - MAX_PERSISTED_SESSIONS;
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
            chunks: vec![Chunk {
                path: "a.txt".into(),
                offset: 0,
                line_count: 1,
                content: "test".into(),
            }],
            total_bytes: 4,
            files_scanned: 1,
            files_skipped: 0,
            skip_reasons: std::collections::HashMap::new(),
            created_at_unix: unix_now(),
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
}