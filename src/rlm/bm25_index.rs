//! Session-scoped BM25 line index with on-disk persistence.
//!
//! Built lazily on first `bm25=true` peek. Invalidated when `session.revision`
//! changes. Stored under `rlm-artifacts/<session_id>/bm25_v1_{cs|ci}.json`.

use crate::error::Result;
use crate::rlm::bm25::{tokenize, Bm25Scorer};
use crate::rlm::chunk_store;
use crate::rlm::session::{Chunk, ScanSession};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

const INDEX_VERSION: u32 = 1;
const MEMORY_CACHE_CAP: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bm25LineDoc {
    pub chunk_id: String,
    pub path: String,
    pub chunk_offset: usize,
    pub line_idx: usize,
    pub line_no: usize,
    pub tokens: Vec<String>,
    /// Line text for cheap previews; full chunk is only resolved for context radius.
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedBm25Index {
    pub version: u32,
    pub session_id: String,
    pub revision: u64,
    pub case_sensitive: bool,
    pub avgdl: f64,
    pub n_docs: usize,
    pub doc_freq: HashMap<String, usize>,
    /// term → document indices into `docs`
    pub postings: HashMap<String, Vec<u32>>,
    pub docs: Vec<Bm25LineDoc>,
    pub bytes_indexed: usize,
    pub chunks_indexed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexSource {
    Memory,
    Disk,
    Built,
}

impl IndexSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Disk => "disk",
            Self::Built => "built",
        }
    }

    pub fn is_hit(self) -> bool {
        matches!(self, Self::Memory | Self::Disk)
    }
}

#[derive(Debug, Clone)]
pub struct Bm25SearchHit {
    pub score: f64,
    pub doc: Bm25LineDoc,
}

type CacheKey = (String, u64, bool);

fn memory_cache() -> &'static Mutex<HashMap<CacheKey, Arc<PersistedBm25Index>>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, Arc<PersistedBm25Index>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn index_file_name(case_sensitive: bool) -> &'static str {
    if case_sensitive {
        "bm25_v1_cs.json"
    } else {
        "bm25_v1_ci.json"
    }
}

pub fn index_path(session_id: &str, case_sensitive: bool) -> PathBuf {
    crate::rlm::artifacts::artifacts_dir(session_id).join(index_file_name(case_sensitive))
}

fn cache_key(session_id: &str, revision: u64, case_sensitive: bool) -> CacheKey {
    (session_id.to_string(), revision, case_sensitive)
}

fn cache_get(
    session_id: &str,
    revision: u64,
    case_sensitive: bool,
) -> Option<Arc<PersistedBm25Index>> {
    memory_cache()
        .lock()
        .ok()?
        .get(&cache_key(session_id, revision, case_sensitive))
        .cloned()
}

fn cache_put(index: Arc<PersistedBm25Index>) {
    if let Ok(mut guard) = memory_cache().lock() {
        if guard.len() >= MEMORY_CACHE_CAP {
            guard.clear();
        }
        let key = cache_key(&index.session_id, index.revision, index.case_sensitive);
        guard.insert(key, index);
    }
}

/// Drop memory + disk indexes for a session (called on session delete).
pub fn invalidate_session(session_id: &str) {
    if let Ok(mut guard) = memory_cache().lock() {
        guard.retain(|(sid, _, _), _| sid != session_id);
    }
    for cs in [true, false] {
        let path = index_path(session_id, cs);
        let _ = std::fs::remove_file(path);
    }
}

fn load_from_disk(
    session_id: &str,
    revision: u64,
    case_sensitive: bool,
) -> Result<Option<PersistedBm25Index>> {
    let path = index_path(session_id, case_sensitive);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let index: PersistedBm25Index = serde_json::from_str(&raw)?;
    if index.version != INDEX_VERSION
        || index.session_id != session_id
        || index.revision != revision
        || index.case_sensitive != case_sensitive
    {
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }
    Ok(Some(index))
}

fn save_to_disk(index: &PersistedBm25Index) -> Result<()> {
    let dir = crate::rlm::artifacts::artifacts_dir(&index.session_id);
    std::fs::create_dir_all(&dir)?;
    let path = index_path(&index.session_id, index.case_sensitive);
    let tmp = dir.join(format!(
        "{}.{}.tmp",
        index_file_name(index.case_sensitive),
        uuid::Uuid::new_v4()
    ));
    let body = serde_json::to_string(index)?;
    std::fs::write(&tmp, body.as_bytes())?;
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
    Ok(())
}

/// Build a full-session line BM25 index (all chunks; filters applied at query time).
pub fn build_index(session: &ScanSession, case_sensitive: bool) -> Result<PersistedBm25Index> {
    let mut docs: Vec<Bm25LineDoc> = Vec::new();
    let mut bytes_indexed = 0usize;
    let mut chunks_indexed = 0usize;

    for chunk in &session.chunks {
        let content = match chunk_store::resolve_content(&session.id, chunk) {
            Ok(c) => c,
            Err(_) => continue,
        };
        chunks_indexed += 1;
        bytes_indexed += content.len();
        for (i, line) in content.lines().enumerate() {
            let tokens = tokenize(line, case_sensitive);
            docs.push(Bm25LineDoc {
                chunk_id: chunk.id.clone(),
                path: chunk.path.clone(),
                chunk_offset: chunk.offset,
                line_idx: i,
                line_no: chunk.offset + i + 1,
                tokens,
                line: line.to_string(),
            });
        }
    }

    let token_docs: Vec<Vec<String>> = docs.iter().map(|d| d.tokens.clone()).collect();
    let scorer = Bm25Scorer::from_documents(&token_docs);

    let mut postings: HashMap<String, Vec<u32>> = HashMap::new();
    for (idx, doc) in docs.iter().enumerate() {
        let mut seen = HashSet::new();
        for tok in &doc.tokens {
            if seen.insert(tok.as_str()) {
                postings.entry(tok.clone()).or_default().push(idx as u32);
            }
        }
    }

    Ok(PersistedBm25Index {
        version: INDEX_VERSION,
        session_id: session.id.clone(),
        revision: session.revision,
        case_sensitive,
        avgdl: scorer.avgdl(),
        n_docs: scorer.n_docs(),
        doc_freq: scorer.doc_freq().clone(),
        postings,
        docs,
        bytes_indexed,
        chunks_indexed,
    })
}

/// Load from memory/disk or build+persist.
pub fn get_or_build(
    session: &ScanSession,
    case_sensitive: bool,
) -> Result<(Arc<PersistedBm25Index>, IndexSource)> {
    if let Some(hit) = cache_get(&session.id, session.revision, case_sensitive) {
        return Ok((hit, IndexSource::Memory));
    }
    if let Some(disk) = load_from_disk(&session.id, session.revision, case_sensitive)? {
        let arc = Arc::new(disk);
        cache_put(Arc::clone(&arc));
        return Ok((arc, IndexSource::Disk));
    }
    let built = build_index(session, case_sensitive)?;
    // Persist best-effort; search still works if write fails.
    let _ = save_to_disk(&built);
    let arc = Arc::new(built);
    cache_put(Arc::clone(&arc));
    Ok((arc, IndexSource::Built))
}

fn path_matches(path: &str, path_filter: Option<&str>, glob: Option<&str>) -> bool {
    if let Some(filter) = path_filter {
        if !path.contains(filter) {
            return false;
        }
    }
    if let Some(pattern) = glob {
        if !crate::rlm::glob_match(pattern, path) {
            return false;
        }
    }
    true
}

/// Score lines using inverted postings + global IDF from the full index.
pub fn search(
    index: &PersistedBm25Index,
    query_tokens: &[String],
    path_filter: Option<&str>,
    glob: Option<&str>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    limit: usize,
) -> (Vec<Bm25SearchHit>, usize /* candidates_scored */) {
    if query_tokens.is_empty() || index.docs.is_empty() {
        return (Vec::new(), 0);
    }

    let mut candidate_ids: HashSet<u32> = HashSet::new();
    for q in query_tokens {
        if let Some(posting) = index.postings.get(q) {
            for &id in posting {
                candidate_ids.insert(id);
            }
        }
    }

    let scorer = Bm25Scorer::from_parts(index.doc_freq.clone(), index.avgdl, index.n_docs);
    let mut scored: Vec<(usize, f64)> = Vec::new();

    for id in candidate_ids {
        let idx = id as usize;
        let doc = match index.docs.get(idx) {
            Some(d) => d,
            None => continue,
        };
        if !path_matches(&doc.path, path_filter, glob) {
            continue;
        }
        if let Some(start) = line_start {
            if doc.line_no < start {
                continue;
            }
        }
        if let Some(end) = line_end {
            if doc.line_no > end {
                continue;
            }
        }
        let score = scorer.score(query_tokens, &doc.tokens);
        if score > 0.0 {
            scored.push((idx, score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let candidates_scored = scored.len();
    let hits = scored
        .into_iter()
        .take(limit)
        .map(|(idx, score)| Bm25SearchHit {
            score,
            doc: index.docs[idx].clone(),
        })
        .collect();
    (hits, candidates_scored)
}

fn preview_with_context(lines: &[&str], center: usize, radius: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let start = center.saturating_sub(radius);
    let end = (center + radius).min(lines.len().saturating_sub(1));
    lines[start..=end].join("\n")
}

/// Resolve preview text with optional context radius (loads chunk body only when radius > 0).
pub fn preview_for_hit(
    session_id: &str,
    chunk: Option<&Chunk>,
    hit: &Bm25SearchHit,
    context_radius: usize,
) -> (String, usize /* bytes_read */) {
    if context_radius == 0 {
        return (hit.doc.line.clone(), 0);
    }
    let Some(chunk) = chunk else {
        return (hit.doc.line.clone(), 0);
    };
    match chunk_store::resolve_content(session_id, chunk) {
        Ok(body) => {
            let bytes = body.len();
            let lines: Vec<&str> = body.lines().collect();
            let preview = preview_with_context(&lines, hit.doc.line_idx, context_radius);
            (preview, bytes)
        }
        Err(_) => (hit.doc.line.clone(), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rlm::session::Chunk;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn session_with(content: &str) -> ScanSession {
        ScanSession {
            id: format!("bm25-idx-{}", uuid::Uuid::new_v4()),
            root_path: "test".into(),
            source_kind: "text".into(),
            chunks: vec![Chunk {
                id: "c-0".into(),
                path: "doc.txt".into(),
                offset: 0,
                line_count: content.lines().count(),
                content: content.into(),
                content_file: None,
            }],
            files_scanned: 1,
            files_skipped: 0,
            skip_reasons: HashMap::new(),
            total_bytes: content.len(),
            variables: HashMap::new(),
            created_at_unix: 0,
            expires_at_unix: 0,
            revision: 3,
        }
    }

    #[test]
    fn build_search_ranks_needle() {
        let session = session_with(
            "filler alpha beta gamma\nNEEDLE_KEY=MAGIC-42\nfiller delta epsilon zeta\n",
        );
        let index = build_index(&session, false).unwrap();
        let q = tokenize("needle key magic", false);
        let (hits, scored) = search(&index, &q, None, None, None, None, 5);
        assert!(scored >= 1);
        assert!(!hits.is_empty());
        assert!(hits[0].doc.line.to_lowercase().contains("needle"));
    }

    #[test]
    fn disk_round_trip_is_hit() {
        let _guard = crate::test_lock::acquire();
        let tmp = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", tmp.path());
        let session = session_with("hello world line\nsecond needle line here\n");
        let (first, src1) = get_or_build(&session, false).unwrap();
        assert_eq!(src1, IndexSource::Built);
        // Clear memory so second load is from disk
        invalidate_session(&session.id);
        // Rebuild once, then drop memory only:
        let (built, _) = get_or_build(&session, false).unwrap();
        drop(built);
        if let Ok(mut g) = memory_cache().lock() {
            g.clear();
        }
        let (_second, src2) = get_or_build(&session, false).unwrap();
        assert_eq!(src2, IndexSource::Disk);
        assert!(index_path(&session.id, false).exists());
        let _ = first;
        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn revision_mismatch_rebuilds() {
        let _guard = crate::test_lock::acquire();
        let tmp = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", tmp.path());
        let mut session = session_with("alpha beta\n");
        session.revision = 1;
        let _ = get_or_build(&session, false).unwrap();
        session.revision = 2;
        let (idx, src) = get_or_build(&session, false).unwrap();
        assert_eq!(src, IndexSource::Built);
        assert_eq!(idx.revision, 2);
        std::env::remove_var("RLM_CACHE_DIR");
    }
}
