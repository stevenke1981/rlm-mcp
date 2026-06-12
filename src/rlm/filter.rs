use crate::rlm::session::{Chunk, ScanSession};
use regex::Regex;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct PeekOptions<'a> {
    pub query: Option<&'a str>,
    pub path_filter: Option<&'a str>,
    pub glob: Option<&'a str>,
    pub regex: bool,
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
        "query": opts.query,
        "path_filter": opts.path_filter,
        "glob": opts.glob,
        "regex": opts.regex,
        "case_sensitive": opts.case_sensitive,
        "total_match_lines": total_matches,
        "returned": results.len(),
        "truncated": total_matches > results.len(),
        "file_summary": file_summary,
        "matches": results,
        "hint": "Feed chunk_id values into rlm_chunk or rlm_map_plan"
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
