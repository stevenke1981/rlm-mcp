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
pub enum OolongSize {
    Mini,
    Small,
}

impl OolongSize {
    pub fn parse_size(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mini" => Some(Self::Mini),
            "small" => Some(Self::Small),
            _ => None,
        }
    }

    fn doc_count(self) -> usize {
        match self {
            Self::Mini => 6,
            Self::Small => 15,
        }
    }

    fn filler_lines_per_doc(self) -> usize {
        match self {
            Self::Mini => 24,
            Self::Small => 30,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OolongFixture {
    pub id: String,
    pub corpus: String,
    pub doc_count: usize,
    pub metric_key: String,
    pub expected_sum: String,
}

pub fn generate_fixture(size: OolongSize) -> OolongFixture {
    let doc_count = size.doc_count();
    let filler = size.filler_lines_per_doc();
    let metric_key = "METRIC".to_string();
    let mut sum = 0u64;
    let mut docs = Vec::with_capacity(doc_count);

    for i in 0..doc_count {
        let value = (i as u64 + 1) * 7;
        sum += value;
        let mut lines = Vec::with_capacity(filler + 2);
        lines.push(format!("=== DOC {i:02} ==="));
        for j in 0..filler {
            lines.push(format!(
                "doc-{i:02}-line-{j:02} synthetic aggregation filler without metrics"
            ));
        }
        let metric_line = format!("{metric_key}={value}");
        lines.insert(filler / 2 + 1, metric_line);
        docs.push(lines.join("\n"));
    }

    let id = format!("oolong-{}-{}docs-sum{sum}", size_label(size), doc_count);
    OolongFixture {
        id,
        corpus: docs.join("\n\n"),
        doc_count,
        metric_key,
        expected_sum: sum.to_string(),
    }
}

fn size_label(size: OolongSize) -> &'static str {
    match size {
        OolongSize::Mini => "mini",
        OolongSize::Small => "small",
    }
}

pub fn run(engine: &RlmEngine, size: OolongSize) -> Result<BenchmarkReport> {
    let fixture = generate_fixture(size);
    let mut baselines = Vec::new();

    for kind in BaselineKind::all() {
        baselines.push(run_baseline(engine, &fixture, *kind)?);
    }

    let haystack_lines = fixture.corpus.lines().count();
    let mut report = BenchmarkReport {
        suite: "oolong".into(),
        fixture_id: fixture.id.clone(),
        haystack_bytes: fixture.corpus.len(),
        haystack_lines,
        needle_key: fixture.metric_key.clone(),
        needle_value: fixture.expected_sum.clone(),
        baselines,
        summary: json!({}),
    };
    report.summary = summarize_oolong(&report);
    Ok(report)
}

fn summarize_oolong(report: &BenchmarkReport) -> serde_json::Value {
    let mut base = summarize_report(report);
    if let Some(obj) = base.as_object_mut() {
        obj.insert(
            "task".into(),
            json!({
                "kind": "metric_sum_aggregation",
                "metric_key": report.needle_key,
                "expected_sum": report.needle_value,
            }),
        );
        if let Some(claims) = obj
            .get_mut("qualitative_claims")
            .and_then(|v| v.as_object_mut())
        {
            claims.insert(
                "compaction_incomplete_aggregation".into(),
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
    fixture: &OolongFixture,
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
            let correct = answer == fixture.expected_sum;
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
                expected: fixture.expected_sum.clone(),
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
            expected: fixture.expected_sum.clone(),
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

fn run_direct(fixture: &OolongFixture) -> Result<(String, String, Option<String>, Option<String>)> {
    let answer = sum_metrics(&fixture.corpus).to_string();
    Ok((
        answer,
        fixture.corpus.clone(),
        None,
        Some("Simulates stuffing full corpus for aggregation".into()),
    ))
}

fn run_summary_compaction(
    fixture: &OolongFixture,
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
    let answer = sum_metrics(&compacted).to_string();
    Ok((
        answer,
        compacted,
        None,
        Some(format!(
            "Compaction reads first/last {edge} lines — misses METRIC lines in middle documents"
        )),
    ))
}

fn run_retrieval_peek(
    engine: &RlmEngine,
    fixture: &OolongFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/oolong.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.metric_key),
            bm25: true,
            case_sensitive: false,
            include_content: false,
            limit: fixture.doc_count.max(20),
            ..Default::default()
        },
    )?;

    let evidence = peek_matches_text(&peek);
    let answer = sum_metrics(&evidence).to_string();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("BM25 peek for METRIC lines then sum (model-visible evidence)".into()),
    ))
}

fn run_rlm_no_subcalls(
    engine: &RlmEngine,
    fixture: &OolongFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/oolong.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.metric_key),
            limit: fixture.doc_count.max(20),
            ..Default::default()
        },
    )?;

    let evidence = chunk_evidence_from_peek(engine, &session_id, &peek)?;

    engine.reduce_merge(&[json!({
        "batch_id": "oolong-0",
        "findings": [{
            "summary": format!("aggregate {} across documents", fixture.metric_key),
            "chunk_ids": [],
            "paths": ["benchmark/oolong.txt"]
        }],
        "unresolved": []
    })])?;

    let answer = sum_metrics(&evidence).to_string();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Filter → map (chunks) → reduce merge without recursive sub-calls".into()),
    ))
}

fn run_rlm_with_subcalls(
    engine: &RlmEngine,
    fixture: &OolongFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.corpus),
        Some("benchmark/oolong.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.metric_key),
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
        &format!("sum all {} values across documents", fixture.metric_key),
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

    let answer = sum_metrics(&evidence).to_string();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("Filter → recursive sub-call (mock) → aggregate METRIC sum from evidence".into()),
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

pub fn sum_metrics(text: &str) -> u64 {
    Regex::new(r"METRIC=(\d+)")
        .ok()
        .map(|re| {
            re.captures_iter(text)
                .filter_map(|c| c.get(1))
                .filter_map(|m| m.as_str().parse::<u64>().ok())
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_fixture_sums_metrics() {
        let f = generate_fixture(OolongSize::Mini);
        assert_eq!(sum_metrics(&f.corpus).to_string(), f.expected_sum);
        assert_eq!(f.doc_count, 6);
    }
}
