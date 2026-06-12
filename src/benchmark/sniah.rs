use crate::benchmark::types::{
    summarize_report, BaselineKind, BaselineResult, BenchmarkReport, RunMetrics,
};
use crate::error::Result;
use crate::rlm::{PeekOptions, RlmEngine};
use regex::Regex;
use serde_json::json;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SniahSize {
    Mini,
    Small,
    Large,
    Nightly,
}

impl SniahSize {
    pub fn parse_size(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mini" => Some(Self::Mini),
            "small" => Some(Self::Small),
            "large" => Some(Self::Large),
            "nightly" => Some(Self::Nightly),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Mini, Self::Small, Self::Large, Self::Nightly]
    }

    pub fn label(self) -> &'static str {
        size_label(self)
    }

    pub fn is_ci_default(self) -> bool {
        matches!(self, Self::Mini)
    }

    pub fn is_optional(self) -> bool {
        !self.is_ci_default()
    }

    fn filler_lines(self) -> usize {
        match self {
            Self::Mini => 40,
            Self::Small => 200,
            Self::Large => 2_000,
            Self::Nightly => 8_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SniahFixture {
    pub id: String,
    pub haystack: String,
    pub needle_key: String,
    pub needle_value: String,
    pub needle_line: String,
}

pub fn generate_fixture(size: SniahSize) -> SniahFixture {
    let filler_count = size.filler_lines();
    let needle_value = format!("MAGIC-{}", uuid::Uuid::new_v4().simple());
    let needle_key = "NEEDLE_KEY".to_string();
    let needle_line = format!("{needle_key}={needle_value}");
    let id = format!(
        "sniah-{}-{}",
        size_label(size),
        &needle_value[..8.min(needle_value.len())]
    );

    let mut lines = Vec::with_capacity(filler_count * 2 + 1);
    for i in 0..filler_count {
        lines.push(format!(
            "filler-{i:04} lorem ipsum dolor sit amet consectetur adipiscing elit sed do"
        ));
    }
    lines.push(needle_line.clone());
    for i in filler_count..filler_count * 2 {
        lines.push(format!(
            "filler-{i:04} more haystack text without secrets or keys here"
        ));
    }

    SniahFixture {
        id,
        haystack: lines.join("\n"),
        needle_key,
        needle_value,
        needle_line,
    }
}

fn size_label(size: SniahSize) -> &'static str {
    match size {
        SniahSize::Mini => "mini",
        SniahSize::Small => "small",
        SniahSize::Large => "large",
        SniahSize::Nightly => "nightly",
    }
}

pub fn run(engine: &RlmEngine, size: SniahSize) -> Result<BenchmarkReport> {
    let fixture = generate_fixture(size);
    let mut baselines = Vec::new();

    for kind in BaselineKind::all() {
        baselines.push(run_baseline(engine, &fixture, *kind)?);
    }

    let haystack_lines = fixture.haystack.lines().count();
    let mut report = BenchmarkReport {
        suite: "sniah".into(),
        fixture_id: fixture.id.clone(),
        haystack_bytes: fixture.haystack.len(),
        haystack_lines,
        needle_key: fixture.needle_key.clone(),
        needle_value: fixture.needle_value.clone(),
        baselines,
        summary: json!({}),
    };
    report.summary = summarize_report(&report);
    Ok(report)
}

fn run_baseline(
    engine: &RlmEngine,
    fixture: &SniahFixture,
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
            let correct = answer == fixture.needle_value;
            let mut metrics = if let Some(ref sid) = session_id {
                collect_engine_metrics(engine, sid, started)?
            } else {
                RunMetrics {
                    runtime_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                }
            };
            // Model-visible context bytes (not full external storage load).
            metrics.bytes_in = evidence.len();
            metrics.tokens_est = metrics.tokens_est.max((evidence.len() / 4) as u64);
            Ok(BaselineResult {
                baseline: kind.as_str().into(),
                correct,
                answer,
                expected: fixture.needle_value.clone(),
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
            expected: fixture.needle_value.clone(),
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

fn run_direct(fixture: &SniahFixture) -> Result<(String, String, Option<String>, Option<String>)> {
    let answer = extract_needle_value(&fixture.haystack, &fixture.needle_key).unwrap_or_default();
    Ok((
        answer,
        fixture.haystack.clone(),
        None,
        Some("Simulates stuffing full haystack into model context".into()),
    ))
}

fn run_summary_compaction(
    fixture: &SniahFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let lines: Vec<&str> = fixture.haystack.lines().collect();
    let edge = (lines.len() / 10).max(3);
    let head: Vec<&str> = lines.iter().copied().take(edge).collect();
    let tail: Vec<&str> = lines
        .iter()
        .copied()
        .skip(lines.len().saturating_sub(edge))
        .collect();
    let compacted = head.into_iter().chain(tail).collect::<Vec<_>>().join("\n");
    let answer = extract_needle_value(&compacted, &fixture.needle_key).unwrap_or_default();
    Ok((
        answer,
        compacted,
        None,
        Some(format!(
            "Compaction reads first/last {edge} lines — needle buried at line {}",
            fixture.haystack.lines().count() / 2
        )),
    ))
}

fn run_retrieval_peek(
    engine: &RlmEngine,
    fixture: &SniahFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.haystack),
        Some("benchmark/sniah.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.needle_key),
            bm25: true,
            case_sensitive: false,
            include_content: false,
            limit: 5,
            ..Default::default()
        },
    )?;

    let evidence = peek_matches_text(&peek);
    let answer = extract_needle_value(&evidence, &fixture.needle_key).unwrap_or_default();
    Ok((
        answer,
        evidence,
        Some(session_id),
        Some("BM25 retrieval via rlm_peek --bm25".into()),
    ))
}

fn run_rlm_no_subcalls(
    engine: &RlmEngine,
    fixture: &SniahFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.haystack),
        Some("benchmark/sniah.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.needle_key),
            limit: 5,
            ..Default::default()
        },
    )?;

    let chunk_id = peek["matches"][0]["chunk_id"]
        .as_str()
        .ok_or_else(|| crate::error::Error::Other("peek found no chunk".into()))?
        .to_string();

    let chunk = engine.chunk(
        &session_id,
        None,
        Some(std::slice::from_ref(&chunk_id)),
        0,
        1,
        true,
    )?;

    let content = chunk["chunks"][0]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    engine.reduce_merge(&[json!({
        "batch_id": "sniah-0",
        "findings": [{
            "summary": format!("found {}", fixture.needle_key),
            "chunk_ids": [chunk_id],
            "paths": ["benchmark/sniah.txt"]
        }],
        "unresolved": []
    })])?;

    let answer = extract_needle_value(&content, &fixture.needle_key).unwrap_or_default();
    Ok((
        answer,
        content,
        Some(session_id),
        Some("Filter → map (chunk) → reduce without recursive sub-calls".into()),
    ))
}

fn run_rlm_with_subcalls(
    engine: &RlmEngine,
    fixture: &SniahFixture,
) -> Result<(String, String, Option<String>, Option<String>)> {
    let scan = engine.scan(
        None,
        Some(&fixture.haystack),
        Some("benchmark/sniah.txt"),
        None,
    )?;
    let session_id = scan["session_id"].as_str().unwrap().to_string();

    let peek = engine.peek(
        &session_id,
        PeekOptions {
            query: Some(&fixture.needle_key),
            limit: 5,
            ..Default::default()
        },
    )?;

    let chunk_id = peek["matches"][0]["chunk_id"]
        .as_str()
        .ok_or_else(|| crate::error::Error::Other("peek found no chunk".into()))?
        .to_string();

    let root = engine.task_create(
        &session_id,
        &format!("extract {} value from context", fixture.needle_key),
        std::slice::from_ref(&chunk_id),
        None,
        "mock",
        None,
        None,
        true,
    )?;
    let root_id = root["root_id"].as_str().unwrap();

    engine.task_reduce(root_id)?;

    let chunk = engine.chunk(
        &session_id,
        None,
        Some(std::slice::from_ref(&chunk_id)),
        0,
        1,
        true,
    )?;
    let content = chunk["chunks"][0]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let answer = extract_needle_value(&content, &fixture.needle_key).unwrap_or_default();
    Ok((
        answer,
        content,
        Some(session_id),
        Some(
            "Filter → recursive sub-call (mock) → reduce; accuracy scored on evidence chunk".into(),
        ),
    ))
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

pub fn extract_needle_value(text: &str, key: &str) -> Option<String> {
    let pattern = format!(r"{key}=([A-Za-z0-9-]+)");
    Regex::new(&pattern)
        .ok()?
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_needle_from_line() {
        let v = extract_needle_value("NEEDLE_KEY=MAGIC-ABC123", "NEEDLE_KEY");
        assert_eq!(v.as_deref(), Some("MAGIC-ABC123"));
    }

    #[test]
    fn mini_fixture_buries_needle() {
        let f = generate_fixture(SniahSize::Mini);
        assert!(f.haystack.contains(&f.needle_line));
        assert!(extract_needle_value(&f.haystack, &f.needle_key).is_some());
    }

    #[test]
    fn optional_fixture_sizes_scale_haystack() {
        let mini = generate_fixture(SniahSize::Mini);
        let small = generate_fixture(SniahSize::Small);
        let large = generate_fixture(SniahSize::Large);
        let nightly = generate_fixture(SniahSize::Nightly);

        assert!(mini.haystack.len() < small.haystack.len());
        assert!(small.haystack.len() < large.haystack.len());
        assert!(large.haystack.len() < nightly.haystack.len());
        assert!(nightly.haystack.lines().count() > 15_000);
    }
}
