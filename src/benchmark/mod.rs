mod sniah;
mod types;

pub use sniah::{extract_needle_value, generate_fixture, run as run_sniah, SniahFixture, SniahSize};
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
            }
        ],
        "planned": [
            "browsecomp_plus",
            "oolong",
            "oolong_pairs",
            "codeqa"
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
        other => Err(Error::InvalidArgument(format!(
            "unknown benchmark suite: {other}. Use benchmark list to see available suites."
        ))),
    }
}