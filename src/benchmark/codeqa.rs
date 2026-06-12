use crate::benchmark::types::{
    summarize_report, BaselineKind, BaselineResult, BenchmarkReport, RunMetrics,
};
use crate::error::Result;
use crate::rlm::{PeekOptions, RlmEngine};
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tempfile::TempDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeqaSize {
    Mini,
    Small,
}

impl CodeqaSize {
    pub fn parse_size(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mini" => Some(Self::Mini),
            "small" => Some(Self::Small),
            _ => None,
        }
    }

    fn file_count(self) -> usize {
        match self {
            Self::Mini => 5,
            Self::Small => 12,
        }
    }
}

#[derive(Debug)]
pub struct CodeqaFixture {
    pub id: String,
    pub root: PathBuf,
    pub target_symbol: String,
    pub expected_answer: String,
    pub peek_query: String,
    pub file_count: usize,
    pub corpus_bytes: usize,
    _temp: TempDir,
}

pub fn generate_fixture(size: CodeqaSize) -> Result<CodeqaFixture> {
    let temp = TempDir::new()?;
    let root = temp.path().join("mini-repo");
    fs::create_dir_all(root.join("src"))?;

    let target_symbol = format!("ingest_events_{}", size_label(size));
    let target_file = "src/pipeline.rs";
    let file_count = size.file_count();
    let mut corpus_bytes = 0usize;

    let readme = (0..30)
        .map(|i| format!("readme-line-{i:02} project overview without symbols"))
        .collect::<Vec<_>>()
        .join("\n");
    write_file(&root.join("README.md"), &readme)?;
    corpus_bytes += readme.len();

    for i in 0..file_count {
        let rel = if i == file_count / 2 {
            target_file.to_string()
        } else if i == 0 {
            "src/lib.rs".into()
        } else if i == 1 {
            "src/config.rs".into()
        } else if i == file_count - 1 {
            "src/util.rs".into()
        } else {
            format!("src/module_{i:02}.rs")
        };
        let mut body: Vec<String> = (0..28)
            .map(|j| format!("// {rel} filler-{j:02} impl scaffolding"))
            .collect();
        if rel == target_file {
            body.insert(
                14,
                format!("pub fn {target_symbol}(batch: &[u8]) -> Result<(), ()> {{"),
            );
            body.insert(15, "    Ok(())".into());
            body.insert(16, "}".into());
        }
        let text = body.join("\n");
        corpus_bytes += text.len();
        write_file(&root.join(&rel), &text)?;
    }

    let id = format!(
        "codeqa-{}-{}files-{}",
        size_label(size),
        file_count,
        &target_symbol[..target_symbol.len().min(12)]
    );

    Ok(CodeqaFixture {
        id,
        root,
        target_symbol: target_symbol.clone(),
        expected_answer: target_symbol,
        peek_query: "ingest_events".into(),
        file_count,
        corpus_bytes,
        _temp: temp,
    })
}

fn size_label(size: CodeqaSize) -> &'static str {
    match size {
        CodeqaSize::Mini => "mini",
        CodeqaSize::Small => "small",
    }
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub fn run(engine: &RlmEngine, size: CodeqaSize) -> Result<BenchmarkReport> {
    let fixture = generate_fixture(size)?;
    let corpus = read_corpus(&fixture.root)?;
    let mut baselines = Vec::new();

    for kind in BaselineKind::all() {
        baselines.push(run_baseline(engine, &fixture, &corpus, *kind)?);
    }

    let haystack_lines = corpus.lines().count();
    let mut report = BenchmarkReport {
        suite: "codeqa".into(),
        fixture_id: fixture.id.clone(),
        haystack_bytes: fixture.corpus_bytes,
        haystack_lines,
        needle_key: "target_symbol".into(),
        needle_value: fixture.expected_answer.clone(),
        baselines,
        summary: json!({}),
    };
    report.summary = summarize_codeqa(&report);
    Ok(report)
}

fn summarize_codeqa(report: &BenchmarkReport) -> serde_json::Value {
    let mut base = summarize_report(report);
    if let Some(obj) = base.as_object_mut() {
        obj.insert(
            "task".into(),
            json!({
                "kind": "repository_symbol_lookup",
                "expected_symbol": report.needle_value,
            }),
        );
        if let Some(claims) = obj
            .get_mut("qualitative_claims")
            .and_then(|v| v.as_object_mut())
        {
            claims.insert(
                "compaction_misses_buried_symbol".into(),
                json!(report
                    .baselines
                    .iter()
                    .find(|b| b.baseline == BaselineKind::SummaryCompaction.as_str())
                    .map(|b| !b.correct)
                    .unwrap_or(false)),
            );
        }
    }
    base
}

fn read_corpus(root: &Path) -> Result<String> {
    let mut parts = Vec::new();
    collect_files(root, root, &mut parts)?;
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(parts
        .into_iter()
        .map(|(rel, body)| format!("=== {rel} ===\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let body = fs::read_to_string(&path)?;
            out.push((rel, body));
        }
    }
    Ok(())
}

fn run_baseline(
    engine: &RlmEngine,
    fixture: &CodeqaFixture,
    corpus: &str,
    kind: BaselineKind,
) -> Result<BaselineResult> {
    let started = Instant::now();
    let root = fixture.root.to_string_lossy().to_string();
    let result = match kind {
        BaselineKind::DirectFullContext => run_direct(corpus, &fixture.target_symbol),
        BaselineKind::SummaryCompaction => run_summary_compaction(corpus, &fixture.target_symbol),
        BaselineKind::RetrievalPeek => run_retrieval_peek(engine, fixture, &root),
        BaselineKind::RlmNoSubcalls => run_rlm_no_subcalls(engine, fixture, &root),
        BaselineKind::RlmWithSubcalls => run_rlm_with_subcalls(engine, fixture, &root),
    };

    match result {
        Ok((answer, evidence, session_id, notes)) => {
            let correct = answer == fixture.expected_answer;
            let mut metrics = if let Some(ref sid) = session_id {
                collect_engine_metrics(engine, sid, started)?
            } else {
                RunMetrics {
                    runtime_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                }
            };
            metrics.bytes_in = evidence.len();
            metrics.tokens_est = metrics.tokens_est.max((evidence.len() / 4) as u64);
            Ok(BaselineResult {
                baseline: kind.as_str().into(),
                correct,
                answer,
                expected: fixture.expected_answer.clone(),
                metrics,
                session_id,
                notes,
                error: None,
            })
        }
        Err(e) => Ok(BaselineResult {
            baseline: kind.as_str().into(),
            correct: false,
            answer: String::new(),
            expected: fixture.expected_answer.clone(),
            metrics: RunMetrics {
                runtime_ms: started.elapsed().as_millis() as u64,
                ..Default::default()
            },
            session_id: None,
            notes: None,
            error: Some(e.to_string()),
        }),
    }
}

fn run_direct(
    corpus: &str,
    target: &str,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let answer = extract_symbol(corpus, target).unwrap_or_default();
    Ok((
        answer,
        corpus.to_string(),
        None,
        Some("Simulates stuffing full repository corpus into model context".into()),
    ))
}

fn run_summary_compaction(
    corpus: &str,
    target: &str,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let lines: Vec<&str> = corpus.lines().collect();
    let edge = (lines.len() / 10).max(3);
    let compacted = lines
        .iter()
        .take(edge)
        .chain(lines.iter().skip(lines.len().saturating_sub(edge)))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let answer = extract_symbol(&compacted, target).unwrap_or_default();
    Ok((
        answer,
        compacted,
        None,
        Some(format!(
            "Compaction reads first/last {edge} lines — misses symbol in middle file"
        )),
    ))
}

fn run_retrieval_peek(
    engine: &RlmEngine,
    fixture: &CodeqaFixture,
    root: &str,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(Some(root), None, None, None)?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.peek_query),
            bm25: true,
            case_sensitive: false,
            include_content: false,
            limit: 10,
            ..Default::default()
        },
    )?;

    let evidence = peek_matches_text(&peek);
    let answer = extract_symbol(&evidence, &fixture.target_symbol).unwrap_or_default();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("BM25 peek for function symbol in mini repo".into()),
    ))
}

fn run_rlm_no_subcalls(
    engine: &RlmEngine,
    fixture: &CodeqaFixture,
    root: &str,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(Some(root), None, None, None)?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.peek_query),
            limit: 10,
            ..Default::default()
        },
    )?;

    let evidence = chunk_evidence_from_peek(engine, &session_id, &peek)?;
    engine.reduce_merge(&[json!({
        "batch_id": "codeqa-0",
        "findings": [{
            "summary": format!("located {}", fixture.peek_query),
            "chunk_ids": [],
            "paths": ["src/pipeline.rs"]
        }],
        "unresolved": []
    })])?;

    let answer = extract_symbol(&evidence, &fixture.target_symbol).unwrap_or_default();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Scan repo → peek → chunk → reduce without sub-calls".into()),
    ))
}

fn run_rlm_with_subcalls(
    engine: &RlmEngine,
    fixture: &CodeqaFixture,
    root: &str,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(Some(root), None, None, None)?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.peek_query),
            limit: 10,
            ..Default::default()
        },
    )?;

    let chunk_id = peek["matches"][0]["chunk_id"]
        .as_str()
        .ok_or_else(|| crate::error::Error::Other("peek found no chunk".into()))?
        .to_string();

    let root_task = engine.task_create(
        &session_id,
        &format!("find pub fn symbol matching {}", fixture.peek_query),
        std::slice::from_ref(&chunk_id),
        None,
        "mock",
        None,
        None,
        true,
    )?;
    let root_id = root_task["root_id"].as_str().unwrap();
    engine.task_reduce(root_id)?;

    let evidence = chunk_evidence_from_peek(engine, &session_id, &peek)?;
    let answer = extract_symbol(&evidence, &fixture.target_symbol).unwrap_or_default();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Scan repo → peek → mock sub-call → reduce".into()),
    ))
}

fn chunk_evidence_from_peek(
    engine: &RlmEngine,
    session_id: &str,
    peek: &serde_json::Value,
) -> Result<String> {
    let mut parts = Vec::new();
    let mut seen = HashSet::new();
    if let Some(matches) = peek["matches"].as_array() {
        for m in matches {
            if let Some(chunk_id) = m["chunk_id"].as_str() {
                if !seen.insert(chunk_id) {
                    continue;
                }
                let id = chunk_id.to_string();
                let chunk = engine.chunk(
                    session_id,
                    None,
                    Some(std::slice::from_ref(&id)),
                    0,
                    1,
                    true,
                )?;
                if let Some(content) = chunk["chunks"][0]["content"].as_str() {
                    parts.push(content.to_string());
                }
            }
        }
    }
    if parts.is_empty() {
        return Ok(peek_matches_text(peek));
    }
    Ok(parts.join("\n"))
}

fn collect_engine_metrics(
    engine: &RlmEngine,
    session_id: &str,
    started: Instant,
) -> Result<RunMetrics> {
    let traj = engine.trajectory_get(session_id, "json", true, &[])?;
    let summary = &traj["summary"];
    let budget = engine.budget_status(session_id);

    let tokens = budget["usage"]["tokens_est"]
        .as_u64()
        .or_else(|| summary["total_bytes_in"].as_u64().map(|b| b / 4))
        .unwrap_or(0);

    Ok(RunMetrics {
        runtime_ms: started.elapsed().as_millis() as u64,
        trajectory_events: summary["event_count"].as_u64().unwrap_or(0),
        bytes_in: summary["total_bytes_in"].as_u64().unwrap_or(0) as usize,
        bytes_out: summary["total_bytes_out"].as_u64().unwrap_or(0) as usize,
        chunks_read: summary["chunks_read"].as_u64().unwrap_or(0),
        sub_call_count: summary["sub_call_count"].as_u64().unwrap_or(0),
        tokens_est: tokens,
    })
}

fn peek_matches_text(peek: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(matches) = peek["matches"].as_array() {
        for m in matches {
            if let Some(preview) = m["preview"].as_str() {
                parts.push(preview.to_string());
            }
            if let Some(content) = m["content"].as_str() {
                parts.push(content.to_string());
            }
            if let Some(ctx) = m["context_lines"].as_array() {
                for line in ctx {
                    if let Some(s) = line.as_str() {
                        parts.push(s.to_string());
                    }
                }
            }
        }
    }
    parts.join("\n")
}

pub fn extract_symbol(text: &str, expected: &str) -> Option<String> {
    let pattern = format!(r"pub fn ({expected})\s*\(");
    Regex::new(&pattern)
        .ok()?
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .or_else(|| {
            if text.contains(expected) {
                Some(expected.to_string())
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_repo_contains_target_symbol() {
        let f = generate_fixture(CodeqaSize::Mini).unwrap();
        let corpus = read_corpus(&f.root).unwrap();
        let sym = extract_symbol(&corpus, &f.target_symbol).unwrap();
        assert_eq!(sym, f.expected_answer);
    }
}
