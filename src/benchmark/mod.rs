mod codeqa;
mod oolong;
mod sniah;
mod types;

pub use codeqa::{
    extract_symbol, generate_fixture as generate_codeqa_fixture, run as run_codeqa, CodeqaSize,
};
pub use oolong::{
    generate_fixture as generate_oolong_fixture, run as run_oolong, sum_metrics, OolongSize,
};
pub use sniah::{
    extract_needle_value, generate_fixture, run as run_sniah, SniahFixture, SniahSize,
};
pub use types::{BaselineKind, BaselineResult, BenchmarkReport, RunMetrics};

use crate::error::{Error, Result};
use crate::rlm::RlmEngine;
use serde_json::{json, Value};

pub fn list_suites() -> Value {
    json!({
        "suites": [
            {
                "id": "sniah",
                "name": "S-NIAH (Synthetic Needle In A Haystack)",
                "description": "Buried key-value needle in synthetic haystack; compares direct, compaction, peek, and RLM baselines.",
                "fixture_sizes": ["mini", "small", "large", "nightly"],
                "ci_fixture_sizes": ["mini"],
                "optional_fixture_sizes": ["small", "large", "nightly"],
                "nightly_fixture_sizes": ["large", "nightly"],
                "ci_default": "mini",
                "baselines": BaselineKind::all()
                    .iter()
                    .map(|b| b.as_str())
                    .collect::<Vec<_>>(),
                "metrics": [
                    "accuracy",
                    "bytes_in",
                    "bytes_out",
                    "tokens_est",
                    "runtime_ms",
                    "trajectory_events",
                    "sub_call_count"
                ],
                "offline": true
            },
            {
                "id": "oolong",
                "name": "OOLONG-like metric aggregation",
                "description": "METRIC values scattered across synthetic documents; baselines must sum all metrics.",
                "fixture_sizes": ["mini", "small"],
                "ci_fixture_sizes": ["mini"],
                "ci_default": "mini",
                "baselines": BaselineKind::all()
                    .iter()
                    .map(|b| b.as_str())
                    .collect::<Vec<_>>(),
                "metrics": [
                    "accuracy",
                    "bytes_in",
                    "bytes_out",
                    "tokens_est",
                    "runtime_ms",
                    "trajectory_events",
                    "sub_call_count"
                ],
                "offline": true
            },
            {
                "id": "codeqa",
                "name": "CodeQA-style repository symbol lookup",
                "description": "Synthetic mini-repo on disk; baselines must find a pub fn symbol in src/pipeline.rs.",
                "fixture_sizes": ["mini", "small"],
                "ci_fixture_sizes": ["mini"],
                "ci_default": "mini",
                "baselines": BaselineKind::all()
                    .iter()
                    .map(|b| b.as_str())
                    .collect::<Vec<_>>(),
                "metrics": [
                    "accuracy",
                    "bytes_in",
                    "bytes_out",
                    "tokens_est",
                    "runtime_ms",
                    "trajectory_events",
                    "sub_call_count"
                ],
                "offline": true
            }
        ],
        "planned": [
            "browsecomp_plus",
            "oolong_pairs"
        ]
    })
}

pub fn run_suite(engine: &RlmEngine, suite: &str, fixture_size: Option<&str>) -> Result<Value> {
    match suite.to_lowercase().as_str() {
        "sniah" => {
            let size = fixture_size
                .and_then(SniahSize::parse_size)
                .unwrap_or(SniahSize::Mini);
            let report = run_sniah(engine, size)?;
            Ok(report.to_value())
        }
        "oolong" => {
            let size = fixture_size
                .and_then(OolongSize::parse_size)
                .unwrap_or(OolongSize::Mini);
            let report = run_oolong(engine, size)?;
            Ok(report.to_value())
        }
        "codeqa" => {
            let size = fixture_size
                .and_then(CodeqaSize::parse_size)
                .unwrap_or(CodeqaSize::Mini);
            let report = run_codeqa(engine, size)?;
            Ok(report.to_value())
        }
        other => Err(Error::InvalidArgument(format!(
            "unknown benchmark suite: {other}. Use benchmark list to see available suites."
        ))),
    }
}
