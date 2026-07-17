//! BrowseComp-Plus-like local multi-document corpus QA.
//!
//! Synthetic "pages" form a small web-like corpus. One page holds a buried fact;
//! head/tail compaction misses it; peek/RLM recover via keyword filter.

use crate::benchmark::types::{
    summarize_report, BaselineKind, BaselineResult, BenchmarkReport, RunMetrics,
};
use crate::error::Result;
use crate::rlm::{PeekOptions, RlmEngine};
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowsecompSize {
    Mini,
    Small,
}

impl BrowsecompSize {
    pub fn parse_size(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mini" => Some(Self::Mini),
            "small" => Some(Self::Small),
            _ => None,
        }
    }

    fn page_count(self) -> usize {
        match self {
            Self::Mini => 8,
            Self::Small => 16,
        }
    }

    fn filler_lines_per_page(self) -> usize {
        match self {
            Self::Mini => 20,
            Self::Small => 28,
        }
    }

    /// 0-based page index that holds the answer fact.
    fn answer_page(self) -> usize {
        match self {
            Self::Mini => 4,
            Self::Small => 9,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowsecompFixture {
    pub id: String,
    pub corpus: String,
    pub page_count: usize,
    pub fact_key: String,
    pub fact_value: String,
    pub query: String,
}

pub fn generate_fixture(size: BrowsecompSize) -> BrowsecompFixture {
    let page_count = size.page_count();
    let filler = size.filler_lines_per_page();
    let answer_page = size.answer_page();
    let fact_key = "BROWSE_FACT".to_string();
    let fact_value = format!("MAGIC-BC-{}", page_count * 17 + answer_page);
    let query = fact_key.clone();

    let mut pages = Vec::with_capacity(page_count);
    for i in 0..page_count {
        let mut lines = Vec::with_capacity(filler + 4);
        lines.push(format!("=== PAGE {i:02} /site/doc-{i}.html ==="));
        lines.push(format!("title: Synthetic BrowseComp page {i}"));
        for j in 0..filler {
            lines.push(format!(
                "page-{i:02}-line-{j:02} lorem distractor content without the target fact"
            ));
        }
        if i == answer_page {
            let fact_line = format!("{fact_key}={fact_value}");
            lines.insert(filler / 2 + 2, fact_line);
            lines.insert(
                filler / 2 + 3,
                "supporting prose around the answer fact for multi-document QA".into(),
            );
        } else {
            // Plausible distractors that must not match FACT= value
            lines.insert(
                filler / 2 + 2,
                format!("OTHER_FACT=noise-{i}-not-the-answer"),
            );
        }
        pages.push(lines.join("\n"));
    }

    let id = format!(
        "browsecomp-{}-{}pages-answer{answer_page}",
        size_label(size),
        page_count
    );
    BrowsecompFixture {
        id,
        corpus: pages.join("\n\n"),
        page_count,
        fact_key,
        fact_value,
        query,
    }
}

fn size_label(size: BrowsecompSize) -> &'static str {
    match size {
        BrowsecompSize::Mini => "mini",
        BrowsecompSize::Small => "small",
    }
}

pub fn extract_fact_value(text: &str) -> Option<String> {
    Regex::new(r"BROWSE_FACT=([A-Za-z0-9\-]+)")
        .ok()?
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

pub fn run(engine: &RlmEngine, size: BrowsecompSize) -> Result<BenchmarkReport> {
    let fixture = generate_fixture(size);
    let mut baselines = Vec::new();

    for kind in BaselineKind::all() {
        baselines.push(run_baseline(engine, &fixture, *kind)?);
    }

    let haystack_lines = fixture.corpus.lines().count();
    let mut report = BenchmarkReport {
        suite: "browsecomp_plus".into(),
        fixture_id: fixture.id.clone(),
        haystack_bytes: fixture.corpus.len(),
        haystack_lines,
        needle_key: fixture.fact_key.clone(),
        needle_value: fixture.fact_value.clone(),
        baselines,
        summary: json!({}),
    };
    report.summary = summarize_browsecomp(&report);
    Ok(report)
}

fn summarize_browsecomp(report: &BenchmarkReport) -> serde_json::Value {
    let mut base = summarize_report(report);
    if let Some(obj) = base.as_object_mut() {
        obj.insert(
            "task".into(),
            json!({
                "kind": "multi_document_fact_lookup",
                "fact_key": report.needle_key,
                "expected_value": report.needle_value,
            }),
        );
        if let Some(claims) = obj
            .get_mut("qualitative_claims")
            .and_then(|v| v.as_object_mut())
        {
            claims.insert(
                "compaction_misses_middle_page_fact".into(),
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
    fixture: &BrowsecompFixture,
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
            let correct = answer == fixture.fact_value;
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
                expected: fixture.fact_value.clone(),
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
            expected: fixture.fact_value.clone(),
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
    fixture: &BrowsecompFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let answer = extract_fact_value(&fixture.corpus).unwrap_or_default();
    Ok((
        answer,
        fixture.corpus.clone(),
        None,
        Some("Simulates stuffing full multi-page corpus into context".into()),
    ))
}

fn run_summary_compaction(
    fixture: &BrowsecompFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let lines: Vec<&str> = fixture.corpus.lines().collect();
    let edge = (lines.len() / 10).max(4);
    let compacted = lines
        .iter()
        .take(edge)
        .chain(lines.iter().skip(lines.len().saturating_sub(edge)))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let answer = extract_fact_value(&compacted).unwrap_or_default();
    Ok((
        answer,
        compacted,
        None,
        Some(format!(
            "Compaction reads first/last {edge} lines — misses fact on middle pages"
        )),
    ))
}

fn run_retrieval_peek(
    engine: &RlmEngine,
    fixture: &BrowsecompFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/browsecomp.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.query),
            bm25: true,
            case_sensitive: false,
            include_content: false,
            limit: 10,
            ..Default::default()
        },
    )?;

    let evidence = peek_matches_text(&peek);
    let answer = extract_fact_value(&evidence).unwrap_or_default();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("BM25/keyword peek for BROWSE_FACT then extract value".into()),
    ))
}

fn run_rlm_no_subcalls(
    engine: &RlmEngine,
    fixture: &BrowsecompFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/browsecomp.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.query),
            limit: 10,
            ..Default::default()
        },
    )?;

    let evidence = chunk_evidence_from_peek(engine, &session_id, &peek)?;
    engine.reduce_merge(&[json!({
        "batch_id": "browsecomp-0",
        "findings": [{
            "summary": format!("locate {}", fixture.fact_key),
            "chunk_ids": [],
            "paths": ["benchmark/browsecomp.txt"]
        }],
        "unresolved": []
    })])?;

    let answer = extract_fact_value(&evidence).unwrap_or_default();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Filter → chunk evidence → reduce without recursive sub-calls".into()),
    ))
}

fn run_rlm_with_subcalls(
    engine: &RlmEngine,
    fixture: &BrowsecompFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/browsecomp.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.query),
            limit: 10,
            ..Default::default()
        },
    )?;

    let chunk_id = peek["matches"][0]["chunk_id"]
        .as_str()
        .ok_or_else(|| crate::error::Error::Other("peek found no chunk".into()))?
        .to_string();

    let root = engine.task_create(
        &session_id,
        &format!(
            "extract {} value from multi-document corpus",
            fixture.fact_key
        ),
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
    let answer = extract_fact_value(&evidence).unwrap_or_default();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Filter → recursive sub-call (mock) → extract fact from evidence".into()),
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
    fn mini_fixture_buries_fact_on_middle_page() {
        let f = generate_fixture(BrowsecompSize::Mini);
        assert_eq!(
            extract_fact_value(&f.corpus).as_deref(),
            Some(f.fact_value.as_str())
        );
        assert!(f.corpus.contains(&f.fact_value));
        let lines: Vec<&str> = f.corpus.lines().collect();
        let edge = (lines.len() / 10).max(4);
        let head_tail: String = lines
            .iter()
            .take(edge)
            .chain(lines.iter().skip(lines.len().saturating_sub(edge)))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(extract_fact_value(&head_tail).is_none());
    }
}
