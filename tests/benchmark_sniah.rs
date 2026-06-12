use rlm_mcp::benchmark::{run_suite, BaselineKind, SniahSize};
use rlm_mcp::rlm::RlmEngine;
use rlm_mcp::test_lock;
use tempfile::TempDir;

fn with_cache<F: FnOnce(&RlmEngine)>(f: F) {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    f(&RlmEngine::new());
    std::env::remove_var("RLM_CACHE_DIR");
}

fn baseline(report: &serde_json::Value, kind: BaselineKind) -> &serde_json::Value {
    report["baselines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["baseline"].as_str() == Some(kind.as_str()))
        .unwrap_or_else(|| panic!("missing baseline {}", kind.as_str()))
}

fn assert_sniah_suite(report: &serde_json::Value, size: &str) {
    assert_eq!(report["suite"].as_str().unwrap(), "sniah");
    assert_eq!(report["baselines"].as_array().unwrap().len(), 5);

    assert!(baseline(report, BaselineKind::DirectFullContext)["correct"]
        .as_bool()
        .unwrap());
    assert!(
        !baseline(report, BaselineKind::SummaryCompaction)["correct"]
            .as_bool()
            .unwrap()
    );

    for kind in [
        BaselineKind::RetrievalPeek,
        BaselineKind::RlmNoSubcalls,
        BaselineKind::RlmWithSubcalls,
    ] {
        let b = baseline(report, kind);
        assert!(
            b["correct"].as_bool().unwrap(),
            "{} ({size}) failed: {:?}",
            kind.as_str(),
            b
        );
        assert!(b["session_id"].as_str().is_some());
    }

    let peek_bytes = baseline(report, BaselineKind::RetrievalPeek)["metrics"]["bytes_in"]
        .as_u64()
        .unwrap();
    let direct_bytes = baseline(report, BaselineKind::DirectFullContext)["metrics"]["bytes_in"]
        .as_u64()
        .unwrap();
    assert!(peek_bytes < direct_bytes);

    let summary = &report["summary"];
    assert_eq!(summary["accuracy"]["correct"].as_u64().unwrap(), 4);
    assert!(
        summary["qualitative_claims"]["retrieval_lower_cost_than_direct"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn sniah_mini_suite_runs_all_baselines() {
    with_cache(|engine| {
        let report = run_suite(engine, "sniah", Some("mini")).unwrap();
        assert_sniah_suite(&report, "mini");
    });
}

#[test]
#[ignore = "local optional: cargo test sniah_small_suite -- --ignored"]
fn sniah_small_suite_runs_all_baselines() {
    with_cache(|engine| {
        let report = run_suite(engine, "sniah", Some("small")).unwrap();
        assert_sniah_suite(&report, "small");
    });
}

#[test]
#[ignore = "local optional: cargo test sniah_large_suite -- --ignored"]
fn sniah_large_suite_runs_all_baselines() {
    with_cache(|engine| {
        let report = run_suite(engine, "sniah", Some("large")).unwrap();
        assert_sniah_suite(&report, "large");
        assert!(report["haystack_lines"].as_u64().unwrap() > 3_000);
    });
}

#[test]
#[ignore = "nightly optional: cargo test sniah_nightly_suite -- --ignored"]
fn sniah_nightly_suite_runs_all_baselines() {
    with_cache(|engine| {
        let report = run_suite(engine, "sniah", Some("nightly")).unwrap();
        assert_sniah_suite(&report, "nightly");
        assert!(report["haystack_lines"].as_u64().unwrap() > 15_000);
    });
}

#[test]
fn sniah_generates_buried_needle_fixture() {
    use rlm_mcp::benchmark::generate_fixture;
    let f = generate_fixture(SniahSize::Mini);
    let lines: Vec<_> = f.haystack.lines().collect();
    let needle_idx = lines
        .iter()
        .position(|l| l.contains(&f.needle_line))
        .unwrap();
    assert!(needle_idx > 5);
    assert!(needle_idx < lines.len() - 5);
}
