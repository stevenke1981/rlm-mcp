use rlm_mcp::benchmark::{
    generate_oolong_pairs_fixture, pair_count_from_text, run_suite, BaselineKind, OolongPairsSize,
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
fn oolong_pairs_mini_suite_runs_all_baselines() {
    with_cache(|engine| {
        let report = run_suite(engine, "oolong_pairs", Some("mini")).unwrap();
        assert_eq!(report["suite"].as_str().unwrap(), "oolong_pairs");
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
            summary["qualitative_claims"]["compaction_incomplete_pairwise"]
                .as_bool()
                .unwrap()
        );
    });
}

#[test]
fn oolong_pairs_fixture_pair_count() {
    let f = generate_oolong_pairs_fixture(OolongPairsSize::Mini);
    assert_eq!(
        pair_count_from_text(&f.corpus).to_string(),
        f.expected_pair_count
    );
    assert!(f.doc_count >= 8);
}
