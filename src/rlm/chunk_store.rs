//! Per-session on-disk chunk content store (lazy index).
//!
//! Session JSON holds metadata only; chunk bodies live under
//! `RLM_CACHE_DIR/rlm-chunks/<session_id>/<chunk_id>.txt`.

use crate::error::{Error, Result};
use crate::rlm::session::Chunk;
use std::path::PathBuf;

pub fn chunks_root() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-chunks")
}

pub fn session_chunks_dir(session_id: &str) -> PathBuf {
    chunks_root().join(session_id)
}

/// Write chunk body and return the relative file name stored on the Chunk.
pub fn write_chunk_content(session_id: &str, chunk_id: &str, content: &str) -> Result<String> {
    let dir = session_chunks_dir(session_id);
    std::fs::create_dir_all(&dir)?;
    let file_name = format!("{chunk_id}.txt");
    let path = dir.join(&file_name);
    let tmp = dir.join(format!("{chunk_id}.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, content.as_bytes())?;
    match std::fs::rename(&tmp, &path) {
        Ok(()) => {}
        Err(_e) if cfg!(windows) => {
            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
            std::fs::rename(&tmp, &path)?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(file_name)
}

pub fn read_chunk_file(session_id: &str, file_name: &str) -> Result<String> {
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return Err(Error::InvalidArgument(
            "invalid chunk content file name".into(),
        ));
    }
    let path = session_chunks_dir(session_id).join(file_name);
    if !path.exists() {
        return Err(Error::InvalidArgument(format!(
            "chunk content missing: {file_name}"
        )));
    }
    Ok(std::fs::read_to_string(path)?)
}

/// Resolve chunk body: inline content (legacy) or on-disk lazy file.
pub fn resolve_content(session_id: &str, chunk: &Chunk) -> Result<String> {
    if !chunk.content.is_empty() {
        return Ok(chunk.content.clone());
    }
    if let Some(ref file) = chunk.content_file {
        return read_chunk_file(session_id, file);
    }
    Ok(String::new())
}

/// Spill all inlined chunk bodies to disk and clear `content` in memory/JSON.
pub fn spill_session_chunks(session_id: &str, chunks: &mut [Chunk]) -> Result<()> {
    for chunk in chunks.iter_mut() {
        if chunk.content.is_empty() {
            continue;
        }
        let file_name = write_chunk_content(session_id, &chunk.id, &chunk.content)?;
        chunk.content_file = Some(file_name);
        chunk.content.clear();
        chunk.content.shrink_to_fit();
    }
    Ok(())
}

/// Expand lazy chunks into a self-contained session (export / tests).
pub fn materialize_session_inline(session_id: &str, chunks: &mut [Chunk]) -> Result<()> {
    for chunk in chunks.iter_mut() {
        if chunk.content.is_empty() {
            if let Some(ref file) = chunk.content_file {
                chunk.content = read_chunk_file(session_id, file)?;
            }
        }
        chunk.content_file = None;
    }
    Ok(())
}

pub fn remove_session_chunks(session_id: &str) -> Result<()> {
    let dir = session_chunks_dir(session_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

pub fn ensure_chunks_layout() -> Result<()> {
    std::fs::create_dir_all(chunks_root())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;
    use tempfile::TempDir;

    #[test]
    fn spill_and_resolve_round_trip() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let mut chunks = vec![Chunk {
            id: "c-0".into(),
            path: "a.txt".into(),
            offset: 0,
            line_count: 1,
            content: "hello lazy".into(),
            content_file: None,
        }];
        spill_session_chunks("sess-1", &mut chunks).unwrap();
        assert!(chunks[0].content.is_empty());
        assert_eq!(chunks[0].content_file.as_deref(), Some("c-0.txt"));
        let body = resolve_content("sess-1", &chunks[0]).unwrap();
        assert_eq!(body, "hello lazy");

        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn rejects_path_traversal_file_name() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());
        let err = read_chunk_file("s", "../x.txt").unwrap_err();
        assert!(err.to_string().contains("invalid"));
        std::env::remove_var("RLM_CACHE_DIR");
    }
}
