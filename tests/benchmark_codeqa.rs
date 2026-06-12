use rlm_mcp::benchmark::{
    extract_symbol, generate_codeqa_fixture, run_suite, BaselineKind, CodeqaSize,
};
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

#[test]
fn codeqa_mini_suite_runs_all_baselines() {
    with_cache(|engine| {
        let report = run_suite(engine, "codeqa", Some("mini")).unwrap();
        assert_eq!(report["suite"].as_str().unwrap(), "codeqa");
        assert_eq!(report["baselines"].as_array().unwrap().len(), 5);

        let expected = report["needle_value"].as_str().unwrap();
        assert!(
            baseline(&report, BaselineKind::DirectFullContext)["correct"]
                .as_bool()
                .unwrap()
        );
        assert!(
            !baseline(&report, BaselineKind::SummaryCompaction)["correct"]
                .as_bool()
                .unwrap()
        );

        for kind in [
            BaselineKind::RetrievalPeek,
            BaselineKind::RlmNoSubcalls,
            BaselineKind::RlmWithSubcalls,
        ] {
            let b = baseline(&report, kind);
            assert!(
                b["correct"].as_bool().unwrap(),
                "{} failed: {:?}",
                kind.as_str(),
                b
            );
            assert_eq!(b["answer"].as_str().unwrap(), expected);
            assert!(b["session_id"].as_str().is_some());
        }

        let peek_bytes = baseline(&report, BaselineKind::RetrievalPeek)["metrics"]["bytes_in"]
            .as_u64()
            .unwrap();
        let direct_bytes = baseline(&report, BaselineKind::DirectFullContext)["metrics"]
            ["bytes_in"]
            .as_u64()
            .unwrap();
        assert!(peek_bytes < direct_bytes);

        let summary = &report["summary"];
        assert_eq!(summary["accuracy"]["correct"].as_u64().unwrap(), 4);
        assert!(
            summary["qualitative_claims"]["compaction_misses_buried_symbol"]
                .as_bool()
                .unwrap()
        );
    });
}

#[test]
fn codeqa_fixture_writes_repo_with_symbol() {
    let f = generate_codeqa_fixture(CodeqaSize::Mini).unwrap();
    let pipeline = std::fs::read_to_string(f.root.join("src/pipeline.rs")).unwrap();
    assert!(extract_symbol(&pipeline, &f.target_symbol).is_some());
}
