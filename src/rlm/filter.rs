use crate::rlm::bm25::tokenize;
use crate::rlm::bm25_index;
use crate::rlm::chunk_store;
use crate::rlm::session::{Chunk, ScanSession};
use regex::Regex;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct PeekOptions<'a> {
    pub query: Option<&'a str>,
    pub path_filter: Option<&'a str>,
    pub glob: Option<&'a str>,
    pub regex: bool,
    pub bm25: bool,
    pub case_sensitive: bool,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub context_radius: usize,
    pub limit: usize,
    pub include_content: bool,
}

impl<'a> Default for PeekOptions<'a> {
    fn default() -> Self {
        Self {
            query: None,
            path_filter: None,
            glob: None,
            regex: false,
            bm25: false,
            case_sensitive: true,
            line_start: None,
            line_end: None,
            context_radius: 2,
            limit: 20,
            include_content: false,
        }
    }
}

pub fn peek_session(session: &ScanSession, opts: PeekOptions<'_>) -> Value {
    if opts.bm25 {
        return peek_bm25(session, opts);
    }

    let compiled = opts
        .query
        .filter(|_| opts.regex)
        .and_then(|q| Regex::new(q).ok());

    let mut total_matches = 0usize;
    let mut results = Vec::new();
    let mut chunks_scanned = 0usize;
    let mut bytes_scanned = 0usize;

    for chunk in &session.chunks {
        if !path_matches(chunk, opts.path_filter, opts.glob) {
            continue;
        }

        let content = match chunk_store::resolve_content(&session.id, chunk) {
            Ok(c) => c,
            Err(_) => continue,
        };
        chunks_scanned += 1;
        bytes_scanned += content.len();
        let line_matches = find_line_matches(chunk, &content, &opts, compiled.as_ref());
        total_matches += line_matches.len();

        if line_matches.is_empty() {
            continue;
        }

        for (line_no, preview) in line_matches
            .into_iter()
            .take(opts.limit.saturating_sub(results.len()))
        {
            let mut entry = json!({
                "chunk_id": chunk.id,
                "path": chunk.path,
                "chunk_offset": chunk.offset,
                "line": line_no,
                "preview": preview,
            });
            if opts.include_content {
                entry["content"] = json!(content);
            }
            results.push(entry);
            if results.len() >= opts.limit {
                break;
            }
        }
        if results.len() >= opts.limit {
            break;
        }
    }

    let file_summary = summarize_files(session, &opts);

    json!({
        "session_id": session.id,
        "search_mode": if opts.regex { "regex" } else { "substring" },
        "query": opts.query,
        "path_filter": opts.path_filter,
        "glob": opts.glob,
        "regex": opts.regex,
        "bm25": false,
        "case_sensitive": opts.case_sensitive,
        "total_match_lines": total_matches,
        "returned": results.len(),
        "truncated": total_matches > results.len(),
        "bytes_scanned": bytes_scanned,
        "chunks_scanned": chunks_scanned,
        "file_summary": file_summary,
        "matches": results,
        "hint": "Feed chunk_id values into rlm_chunk or rlm_map_plan"
    })
}

fn peek_bm25(session: &ScanSession, opts: PeekOptions<'_>) -> Value {
    let query_text = opts.query.unwrap_or("");
    let query_tokens = tokenize(query_text, opts.case_sensitive);

    let (index, index_source) = match bm25_index::get_or_build(session, opts.case_sensitive) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "session_id": session.id,
                "search_mode": "bm25",
                "error": e.to_string(),
                "matches": [],
                "returned": 0,
            });
        }
    };

    let (hits, candidates_scored) = bm25_index::search(
        &index,
        &query_tokens,
        opts.path_filter,
        opts.glob,
        opts.line_start,
        opts.line_end,
        opts.limit,
    );

    let total_matches = candidates_scored;
    let mut file_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut results = Vec::new();
    let mut bytes_scanned = 0usize;
    let mut chunks_loaded = 0usize;

    // Build chunk lookup once for optional content / context radius.
    let chunk_by_id: std::collections::HashMap<&str, &Chunk> =
        session.chunks.iter().map(|c| (c.id.as_str(), c)).collect();

    for hit in &hits {
        *file_counts.entry(hit.doc.path.clone()).or_default() += 1;
        let chunk = chunk_by_id.get(hit.doc.chunk_id.as_str()).copied();
        let (preview, preview_bytes) =
            bm25_index::preview_for_hit(&session.id, chunk, hit, opts.context_radius);
        if preview_bytes > 0 {
            bytes_scanned += preview_bytes;
            chunks_loaded += 1;
        }

        let mut entry = json!({
            "chunk_id": hit.doc.chunk_id,
            "path": hit.doc.path,
            "chunk_offset": hit.doc.chunk_offset,
            "line": hit.doc.line_no,
            "preview": preview,
            "bm25_score": (hit.score * 1000.0).round() / 1000.0,
        });
        if opts.include_content {
            if let Some(c) = chunk {
                if let Ok(body) = chunk_store::resolve_content(&session.id, c) {
                    bytes_scanned += body.len();
                    chunks_loaded += 1;
                    entry["content"] = json!(body);
                }
            }
        }
        results.push(entry);
    }

    // First-time build already read the corpus; report indexed size for budget awareness.
    if !index_source.is_hit() {
        bytes_scanned = index.bytes_indexed;
        chunks_loaded = index.chunks_indexed;
    }

    let mut file_summary: Vec<_> = file_counts
        .into_iter()
        .map(|(path, match_count)| json!({ "path": path, "match_count": match_count }))
        .collect();
    file_summary.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or("")
            .cmp(b["path"].as_str().unwrap_or(""))
    });

    json!({
        "session_id": session.id,
        "search_mode": "bm25",
        "query": opts.query,
        "path_filter": opts.path_filter,
        "glob": opts.glob,
        "regex": false,
        "bm25": true,
        "case_sensitive": opts.case_sensitive,
        "total_match_lines": total_matches,
        "returned": results.len(),
        "truncated": total_matches > results.len(),
        "bytes_scanned": bytes_scanned,
        "chunks_scanned": chunks_loaded,
        "lines_indexed": index.docs.len(),
        "candidates_scored": candidates_scored,
        "index_hit": index_source.is_hit(),
        "index_source": index_source.as_str(),
        "index_revision": index.revision,
        "file_summary": file_summary,
        "matches": results,
        "hint": "BM25-ranked lines; feed chunk_id into rlm_chunk or rlm_map_plan"
    })
}

fn path_matches(chunk: &Chunk, path_filter: Option<&str>, glob: Option<&str>) -> bool {
    if let Some(filter) = path_filter {
        if !chunk.path.contains(filter) {
            return false;
        }
    }
    if let Some(pattern) = glob {
        if !super::glob_match(pattern, &chunk.path) {
            return false;
        }
    }
    true
}

fn find_line_matches(
    chunk: &Chunk,
    content: &str,
    opts: &PeekOptions<'_>,
    compiled: Option<&Regex>,
) -> Vec<(usize, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut hits = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line_no = chunk.offset + i + 1;
        if let Some(start) = opts.line_start {
            if line_no < start {
                continue;
            }
        }
        if let Some(end) = opts.line_end {
            if line_no > end {
                continue;
            }
        }

        let matched = match opts.query {
            None => opts.path_filter.is_some() || opts.glob.is_some(),
            Some(_) if opts.regex => compiled.map(|re| re.is_match(line)).unwrap_or(false),
            Some(q) if opts.case_sensitive => line.contains(q) || chunk.path.contains(q),
            Some(q) => {
                line.to_lowercase().contains(&q.to_lowercase())
                    || chunk.path.to_lowercase().contains(&q.to_lowercase())
            }
        };

        if matched {
            hits.push((
                line_no,
                preview_with_context(&lines, i, opts.context_radius),
            ));
        }
    }

    hits
}

fn preview_with_context(lines: &[&str], center: usize, radius: usize) -> String {
    let start = center.saturating_sub(radius);
    let end = (center + radius).min(lines.len().saturating_sub(1));
    lines[start..=end].join("\n")
}

fn summarize_files(session: &ScanSession, opts: &PeekOptions<'_>) -> Vec<Value> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let compiled = opts
        .query
        .filter(|_| opts.regex)
        .and_then(|q| Regex::new(q).ok());

    for chunk in &session.chunks {
        if !path_matches(chunk, opts.path_filter, opts.glob) {
            continue;
        }
        let content = match chunk_store::resolve_content(&session.id, chunk) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let n = find_line_matches(chunk, &content, opts, compiled.as_ref()).len();
        if n > 0 || (opts.query.is_none() && (opts.path_filter.is_some() || opts.glob.is_some())) {
            *counts.entry(chunk.path.clone()).or_default() += n.max(1);
        }
    }

    let mut summary: Vec<_> = counts
        .into_iter()
        .map(|(path, match_count)| json!({ "path": path, "match_count": match_count }))
        .collect();
    summary.sort_by(|a, b| {
        a["path"]
            .as_str()
            .unwrap_or("")
            .cmp(b["path"].as_str().unwrap_or(""))
    });
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rlm::session::{Chunk, ScanSession};
    use std::collections::HashMap;

    fn test_session(content: &str) -> ScanSession {
        ScanSession {
            id: "s1".into(),
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
            revision: 0,
        }
    }

    #[test]
    fn bm25_peek_ranks_needle_line() {
        let _guard = crate::test_lock::acquire();
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", tmp.path());
        let mut session = test_session(
            "filler alpha beta gamma\nNEEDLE_KEY=MAGIC-42\nfiller delta epsilon zeta\n",
        );
        session.id = format!("s-{}", uuid::Uuid::new_v4());
        session.revision = 1;
        let out = peek_session(
            &session,
            PeekOptions {
                query: Some("needle key magic"),
                bm25: true,
                case_sensitive: false,
                limit: 5,
                ..Default::default()
            },
        );
        assert_eq!(out["search_mode"].as_str().unwrap(), "bm25");
        assert!(out["returned"].as_u64().unwrap() >= 1);
        assert_eq!(out["index_source"].as_str().unwrap(), "built");
        let out2 = peek_session(
            &session,
            PeekOptions {
                query: Some("needle key magic"),
                bm25: true,
                case_sensitive: false,
                limit: 5,
                ..Default::default()
            },
        );
        assert!(out2["index_hit"].as_bool().unwrap());
        assert!(
            out2["index_source"].as_str() == Some("memory")
                || out2["index_source"].as_str() == Some("disk")
        );
        std::env::remove_var("RLM_CACHE_DIR");
        let first = &out["matches"][0];
        assert!(first["preview"].as_str().unwrap().contains("NEEDLE_KEY"));
        assert!(first["bm25_score"].as_f64().unwrap() > 0.0);
    }
}
