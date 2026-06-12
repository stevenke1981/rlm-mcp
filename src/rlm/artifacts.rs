use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const LOCK_RETRIES: u32 = 8;
const LOCK_SLEEP_MS: u64 = 25;

pub fn max_artifact_bytes() -> usize {
    std::env::var("RLM_MAX_ARTIFACT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(super::transform::max_output_bytes)
}

pub fn artifacts_dir(session_id: &str) -> PathBuf {
    crate::project::default_cache_dir()
        .join("rlm-artifacts")
        .join(session_id)
}

fn artifact_path(session_id: &str, name: &str) -> Result<PathBuf> {
    let safe = sanitize_name(name)?;
    Ok(artifacts_dir(session_id).join(format!("{safe}.txt")))
}

pub fn sanitize_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidArgument("artifact name required".into()));
    }
    if trimmed.contains("..") || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(Error::InvalidArgument(
            "artifact name must not contain path separators or ..".into(),
        ));
    }
    if trimmed.len() > 128 {
        return Err(Error::InvalidArgument(
            "artifact name too long (max 128)".into(),
        ));
    }
    Ok(trimmed.to_string())
}

struct ArtifactLock {
    path: PathBuf,
}

impl ArtifactLock {
    fn acquire(session_id: &str) -> Result<Self> {
        let dir = artifacts_dir(session_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(".lock");
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
                        return Err(Error::Other(format!("artifact lock busy: {session_id}")));
                    }
                    std::thread::sleep(Duration::from_millis(LOCK_SLEEP_MS));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Other(format!("artifact lock busy: {session_id}")))
    }
}

impl Drop for ArtifactLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("artifact path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Other("invalid artifact path".into()))?;
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

pub fn write_artifact(session_id: &str, name: &str, content: &str) -> Result<Value> {
    let max = max_artifact_bytes();
    if content.len() > max {
        return Err(Error::InvalidArgument(format!(
            "artifact content exceeds max {max} bytes"
        )));
    }
    let _lock = ArtifactLock::acquire(session_id)?;
    let path = artifact_path(session_id, name)?;
    atomic_write(&path, content)?;
    Ok(json!({
        "session_id": session_id,
        "name": sanitize_name(name)?,
        "bytes": content.len(),
        "line_count": content.lines().count(),
        "path": path.to_string_lossy(),
        "hint": "Read back with rlm_artifact_read or chain rlm_transform"
    }))
}

pub fn read_artifact(
    session_id: &str,
    name: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<Value> {
    let path = artifact_path(session_id, name)?;
    if !path.exists() {
        return Err(Error::Other(format!(
            "artifact not found: {}",
            sanitize_name(name)?
        )));
    }
    let content = std::fs::read_to_string(&path)?;
    let max = max_artifact_bytes();
    if content.len() > max {
        return Err(Error::Other(format!(
            "stored artifact exceeds max read size {max}"
        )));
    }

    let slice = if let (Some(start), Some(end)) = (start_line, end_line) {
        let lines: Vec<&str> = content.lines().collect();
        let start_idx = start.saturating_sub(1).min(lines.len());
        let end_idx = end.max(start).min(lines.len());
        if start_idx < end_idx {
            lines[start_idx..end_idx].join("\n")
        } else {
            String::new()
        }
    } else {
        content.clone()
    };

    Ok(json!({
        "session_id": session_id,
        "name": sanitize_name(name)?,
        "bytes": slice.len(),
        "line_count": slice.lines().count(),
        "start_line": start_line,
        "end_line": end_line,
        "content": slice
    }))
}

#[allow(dead_code)]
pub fn list_artifacts(session_id: &str) -> Result<Vec<String>> {
    let dir = artifacts_dir(session_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("txt") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;
    use tempfile::TempDir;

    #[test]
    fn rejects_path_traversal_names() {
        assert!(sanitize_name("../etc/passwd").is_err());
        assert!(sanitize_name("ok-name").is_ok());
    }

    #[test]
    fn write_read_round_trip() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        write_artifact("sess-1", "summary", "line one\nline two\n").unwrap();
        let read = read_artifact("sess-1", "summary", None, None).unwrap();
        assert!(read["content"].as_str().unwrap().contains("line one"));
        assert!(read["content"].as_str().unwrap().contains("line two"));
        assert_eq!(list_artifacts("sess-1").unwrap(), vec!["summary"]);

        std::env::remove_var("RLM_CACHE_DIR");
    }
}
