//! OOLONG-Pairs-like pairwise aggregation over labeled documents.
//!
//! Each document has CATEGORY and VAL. The gold answer is the number of
//! unordered pairs of documents that share the same CATEGORY (pair count).
//! Head/tail compaction under-counts pairs buried in the middle.

use crate::benchmark::types::{
    summarize_report, BaselineKind, BaselineResult, BenchmarkReport, RunMetrics,
};
use crate::error::Result;
use crate::rlm::{PeekOptions, RlmEngine};
use regex::Regex;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OolongPairsSize {
    Mini,
    Small,
}

impl OolongPairsSize {
    pub fn parse_size(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mini" => Some(Self::Mini),
            "small" => Some(Self::Small),
            _ => None,
        }
    }

    fn doc_count(self) -> usize {
        match self {
            Self::Mini => 8,
            Self::Small => 14,
        }
    }

    fn filler_lines_per_doc(self) -> usize {
        match self {
            Self::Mini => 18,
            Self::Small => 24,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OolongPairsFixture {
    pub id: String,
    pub corpus: String,
    pub doc_count: usize,
    pub expected_pair_count: String,
}

/// Categories cycle so mid-corpus docs form multi-way pairs.
fn category_for(i: usize) -> &'static str {
    match i % 3 {
        0 => "red",
        1 => "blue",
        _ => "green",
    }
}

pub fn generate_fixture(size: OolongPairsSize) -> OolongPairsFixture {
    let doc_count = size.doc_count();
    let filler = size.filler_lines_per_doc();
    let mut docs = Vec::with_capacity(doc_count);
    let mut categories: Vec<String> = Vec::with_capacity(doc_count);

    for i in 0..doc_count {
        let cat = category_for(i).to_string();
        let val = (i as u64 + 1) * 3;
        categories.push(cat.clone());
        let mut lines = Vec::with_capacity(filler + 3);
        lines.push(format!("=== DOC {i:02} ==="));
        for j in 0..filler {
            lines.push(format!(
                "doc-{i:02}-line-{j:02} pairwise aggregation filler without labels"
            ));
        }
        // Bury CATEGORY/VAL in the middle of each document.
        lines.insert(filler / 2 + 1, format!("CATEGORY={cat} VAL={val}"));
        docs.push(lines.join("\n"));
    }

    let expected = count_same_category_pairs(&categories);
    let id = format!(
        "oolong-pairs-{}-{}docs-pairs{expected}",
        size_label(size),
        doc_count
    );
    OolongPairsFixture {
        id,
        corpus: docs.join("\n\n"),
        doc_count,
        expected_pair_count: expected.to_string(),
    }
}

fn size_label(size: OolongPairsSize) -> &'static str {
    match size {
        OolongPairsSize::Mini => "mini",
        OolongPairsSize::Small => "small",
    }
}

/// Unordered pairs among documents that share a category: C(n,2) per category.
pub fn count_same_category_pairs(categories: &[String]) -> u64 {
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for c in categories {
        *counts.entry(c.as_str()).or_default() += 1;
    }
    counts
        .values()
        .map(|&n| if n >= 2 { n * (n - 1) / 2 } else { 0 })
        .sum()
}

/// Parse CATEGORY=... lines and compute pair count from a text blob.
pub fn pair_count_from_text(text: &str) -> u64 {
    let re = match Regex::new(r"CATEGORY=([A-Za-z0-9_\-]+)") {
        Ok(r) => r,
        Err(_) => return 0,
    };
    let cats: Vec<String> = re
        .captures_iter(text)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();
    count_same_category_pairs(&cats)
}

pub fn run(engine: &RlmEngine, size: OolongPairsSize) -> Result<BenchmarkReport> {
    let fixture = generate_fixture(size);
    let mut baselines = Vec::new();

    for kind in BaselineKind::all() {
        baselines.push(run_baseline(engine, &fixture, *kind)?);
    }

    let haystack_lines = fixture.corpus.lines().count();
    let mut report = BenchmarkReport {
        suite: "oolong_pairs".into(),
        fixture_id: fixture.id.clone(),
        haystack_bytes: fixture.corpus.len(),
        haystack_lines,
        needle_key: "PAIR_COUNT".into(),
        needle_value: fixture.expected_pair_count.clone(),
        baselines,
        summary: json!({}),
    };
    report.summary = summarize_pairs(&report);
    Ok(report)
}

fn summarize_pairs(report: &BenchmarkReport) -> serde_json::Value {
    let mut base = summarize_report(report);
    if let Some(obj) = base.as_object_mut() {
        obj.insert(
            "task".into(),
            json!({
                "kind": "pairwise_same_category_count",
                "metric": "PAIR_COUNT",
                "expected_pair_count": report.needle_value,
            }),
        );
        if let Some(claims) = obj
            .get_mut("qualitative_claims")
            .and_then(|v| v.as_object_mut())
        {
            claims.insert(
                "compaction_incomplete_pairwise".into(),
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

fn run_baseline(
    engine: &RlmEngine,
    fixture: &OolongPairsFixture,
    kind: BaselineKind,
) -> Result<BaselineResult> {
    let started = Instant::now();
    let result = match kind {
        BaselineKind::DirectFullContext => run_direct(fixture),
        BaselineKind::SummaryCompaction => run_summary_compaction(fixture),
        BaselineKind::RetrievalPeek => run_retrieval_peek(engine, fixture),
        BaselineKind::RlmNoSubcalls => run_rlm_no_subcalls(engine, fixture),
        BaselineKind::RlmWithSubcalls => run_rlm_with_subcalls(engine, fixture),
    };

    match result {
        Ok((answer, evidence, session_id, notes)) => {
            let correct = answer == fixture.expected_pair_count;
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
                expected: fixture.expected_pair_count.clone(),
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
            expected: fixture.expected_pair_count.clone(),
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
    fixture: &OolongPairsFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let answer = pair_count_from_text(&fixture.corpus).to_string();
    Ok((
        answer,
        fixture.corpus.clone(),
        None,
        Some("Simulates full-corpus pairwise aggregation".into()),
    ))
}

fn run_summary_compaction(
    fixture: &OolongPairsFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let lines: Vec<&str> = fixture.corpus.lines().collect();
    let edge = (lines.len() / 10).max(3);
    let compacted = lines
        .iter()
        .take(edge)
        .chain(lines.iter().skip(lines.len().saturating_sub(edge)))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let answer = pair_count_from_text(&compacted).to_string();
    Ok((
        answer,
        compacted,
        None,
        Some(format!(
            "Compaction reads first/last {edge} lines — incomplete CATEGORY coverage"
        )),
    ))
}

fn run_retrieval_peek(
    engine: &RlmEngine,
    fixture: &OolongPairsFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/oolong_pairs.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some("CATEGORY"),
            bm25: true,
            case_sensitive: false,
            include_content: false,
            limit: fixture.doc_count.max(20),
            ..Default::default()
        },
    )?;

    let evidence = peek_matches_text(&peek);
    let answer = pair_count_from_text(&evidence).to_string();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("BM25 peek for CATEGORY lines then pairwise count".into()),
    ))
}

fn run_rlm_no_subcalls(
    engine: &RlmEngine,
    fixture: &OolongPairsFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/oolong_pairs.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some("CATEGORY"),
            limit: fixture.doc_count.max(20),
            ..Default::default()
        },
    )?;

    let evidence = chunk_evidence_from_peek(engine, &session_id, &peek)?;
    engine.reduce_merge(&[json!({
        "batch_id": "oolong-pairs-0",
        "findings": [{
            "summary": "count same-category document pairs",
            "chunk_ids": [],
            "paths": ["benchmark/oolong_pairs.txt"]
        }],
        "unresolved": []
    })])?;

    let answer = pair_count_from_text(&evidence).to_string();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Filter → map chunks → reduce; pairwise count from evidence".into()),
    ))
}

fn run_rlm_with_subcalls(
    engine: &RlmEngine,
    fixture: &OolongPairsFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/oolong_pairs.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some("CATEGORY"),
            limit: fixture.doc_count.max(20),
            ..Default::default()
        },
    )?;

    let chunk_id = peek["matches"][0]["chunk_id"]
        .as_str()
        .ok_or_else(|| crate::error::Error::Other("peek found no chunk".into()))?
        .to_string();

    let root = engine.task_create(
        &session_id,
        "count unordered pairs of documents sharing CATEGORY",
        std::slice::from_ref(&chunk_id),
        None,
        "mock",
        None,
        None,
        true,
    )?;
    let root_id = root["root_id"].as_str().unwrap();
    engine.task_reduce(root_id)?;

    let evidence = chunk_evidence_from_peek(engine, &session_id, &peek)?;
    let answer = pair_count_from_text(&evidence).to_string();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Filter → recursive sub-call (mock) → pairwise count from evidence".into()),
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
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_fixture_pair_count_matches_parser() {
        let f = generate_fixture(OolongPairsSize::Mini);
        assert_eq!(
            pair_count_from_text(&f.corpus).to_string(),
            f.expected_pair_count
        );
        assert!(f.expected_pair_count.parse::<u64>().unwrap() > 0);
    }

    #[test]
    fn pair_math_c_n_2() {
        // 3 red, 2 blue, 1 green -> C(3,2)+C(2,2)+C(1,2) = 3+1+0 = 4
        let cats = vec![
            "red".into(),
            "blue".into(),
            "red".into(),
            "green".into(),
            "red".into(),
            "blue".into(),
        ];
        assert_eq!(count_same_category_pairs(&cats), 4);
    }
}
