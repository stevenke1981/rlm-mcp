use crate::rlm::bm25::{tokenize, Bm25Scorer};
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

    for chunk in &session.chunks {
        if !path_matches(chunk, opts.path_filter, opts.glob) {
            continue;
        }

        let line_matches = find_line_matches(chunk, &opts, compiled.as_ref());
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
                entry["content"] = json!(chunk.content);
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
        "file_summary": file_summary,
        "matches": results,
        "hint": "Feed chunk_id values into rlm_chunk or rlm_map_plan"
    })
}

struct LineCandidate<'a> {
    chunk: &'a Chunk,
    line_idx: usize,
    line_no: usize,
}

fn peek_bm25(session: &ScanSession, opts: PeekOptions<'_>) -> Value {
    let query_text = opts.query.unwrap_or("");
    let query_tokens = tokenize(query_text, opts.case_sensitive);

    let mut candidates = Vec::new();
    for chunk in &session.chunks {
        if !path_matches(chunk, opts.path_filter, opts.glob) {
            continue;
        }
        let lines: Vec<&str> = chunk.content.lines().collect();
        for (i, _line) in lines.iter().enumerate() {
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
            candidates.push(LineCandidate {
                chunk,
                line_idx: i,
                line_no,
            });
        }
    }

    let doc_tokens: Vec<Vec<String>> = candidates
        .iter()
        .map(|c| {
            let line = c.chunk.content.lines().nth(c.line_idx).unwrap_or("");
            tokenize(line, opts.case_sensitive)
        })
        .collect();
    let scorer = Bm25Scorer::from_documents(&doc_tokens);

    let mut scored: Vec<(usize, f64)> = candidates
        .iter()
        .enumerate()
        .map(|(idx, _)| (idx, scorer.score(&query_tokens, &doc_tokens[idx])))
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let total_matches = scored.len();
    let top = scored.into_iter().take(opts.limit).collect::<Vec<_>>();

    let mut file_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut results = Vec::new();

    for (idx, score) in top {
        let c = &candidates[idx];
        let lines: Vec<&str> = c.chunk.content.lines().collect();
        *file_counts.entry(c.chunk.path.clone()).or_default() += 1;
        let mut entry = json!({
            "chunk_id": c.chunk.id,
            "path": c.chunk.path,
            "chunk_offset": c.chunk.offset,
            "line": c.line_no,
            "preview": preview_with_context(&lines, c.line_idx, opts.context_radius),
            "bm25_score": (score * 1000.0).round() / 1000.0,
        });
        if opts.include_content {
            entry["content"] = json!(c.chunk.content);
        }
        results.push(entry);
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
    opts: &PeekOptions<'_>,
    compiled: Option<&Regex>,
) -> Vec<(usize, String)> {
    let lines: Vec<&str> = chunk.content.lines().collect();
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
        let n = find_line_matches(chunk, opts, compiled.as_ref()).len();
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
        let session = test_session(
            "filler alpha beta gamma\nNEEDLE_KEY=MAGIC-42\nfiller delta epsilon zeta\n",
        );
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
        let first = &out["matches"][0];
        assert!(first["preview"].as_str().unwrap().contains("NEEDLE_KEY"));
        assert!(first["bm25_score"].as_f64().unwrap() > 0.0);
    }
}
