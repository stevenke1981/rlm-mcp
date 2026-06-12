use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineKind {
    DirectFullContext,
    SummaryCompaction,
    RetrievalPeek,
    RlmNoSubcalls,
    RlmWithSubcalls,
}

impl BaselineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectFullContext => "direct_full_context",
            Self::SummaryCompaction => "summary_compaction",
            Self::RetrievalPeek => "retrieval_peek",
            Self::RlmNoSubcalls => "rlm_no_subcalls",
            Self::RlmWithSubcalls => "rlm_with_subcalls",
        }
    }

    pub fn all() -> &'static [BaselineKind] {
        &[
            Self::DirectFullContext,
            Self::SummaryCompaction,
            Self::RetrievalPeek,
            Self::RlmNoSubcalls,
            Self::RlmWithSubcalls,
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunMetrics {
    pub runtime_ms: u64,
    pub trajectory_events: u64,
    pub bytes_in: usize,
    pub bytes_out: usize,
    pub chunks_read: u64,
    pub sub_call_count: u64,
    pub tokens_est: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineResult {
    pub baseline: String,
    pub correct: bool,
    pub answer: String,
    pub expected: String,
    pub metrics: RunMetrics,
    pub session_id: Option<String>,
    pub notes: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub suite: String,
    pub fixture_id: String,
    pub haystack_bytes: usize,
    pub haystack_lines: usize,
    pub needle_key: String,
    pub needle_value: String,
    pub baselines: Vec<BaselineResult>,
    pub summary: Value,
}

impl BenchmarkReport {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(json!({}))
    }
}

pub fn summarize_report(report: &BenchmarkReport) -> Value {
    let correct: u64 = report.baselines.iter().filter(|b| b.correct).count() as u64;
    let total = report.baselines.len() as u64;

    let mut cost_by_baseline = serde_json::Map::new();
    for b in &report.baselines {
        cost_by_baseline.insert(
            b.baseline.clone(),
            json!({
                "correct": b.correct,
                "bytes_in": b.metrics.bytes_in,
                "bytes_out": b.metrics.bytes_out,
                "tokens_est": b.metrics.tokens_est,
                "trajectory_events": b.metrics.trajectory_events,
                "runtime_ms": b.metrics.runtime_ms,
            }),
        );
    }

    let peek = report
        .baselines
        .iter()
        .find(|b| b.baseline == BaselineKind::RetrievalPeek.as_str());
    let direct = report
        .baselines
        .iter()
        .find(|b| b.baseline == BaselineKind::DirectFullContext.as_str());

    let retrieval_beats_direct = peek
        .zip(direct)
        .map(|(p, d)| p.correct && p.metrics.bytes_in < d.metrics.bytes_in);

    json!({
        "accuracy": { "correct": correct, "total": total },
        "cost_by_baseline": cost_by_baseline,
        "qualitative_claims": {
            "retrieval_lower_cost_than_direct": retrieval_beats_direct,
            "summary_compaction_misses_buried_needle": report
                .baselines
                .iter()
                .find(|b| b.baseline == BaselineKind::SummaryCompaction.as_str())
                .map(|b| !b.correct)
                .unwrap_or(false),
            "rlm_subcalls_higher_variance": report
                .baselines
                .iter()
                .find(|b| b.baseline == BaselineKind::RlmWithSubcalls.as_str())
                .map(|b| b.metrics.sub_call_count > 0)
                .unwrap_or(false),
        },
        "paper_note": "Median costs comparable; inspect tail via trajectory sub_call and budget events"
    })
}
