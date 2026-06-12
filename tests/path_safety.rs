use rlm_mcp::project;
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

#[test]
fn scan_rejects_parent_dir_path_on_windows() {
    with_cache(|engine| {
        let err = engine
            .scan(Some("examples\\..\\..\\windows"), None, None, None)
            .unwrap_err();
        assert!(err.to_string().contains(".."));
    });
}

#[test]
fn scan_rejects_traversal_in_middle_of_path() {
    with_cache(|engine| {
        let err = engine
            .scan(Some("examples/fixtures/..\\..\\secret"), None, None, None)
            .unwrap_err();
        assert!(err.to_string().contains(".."));
    });
}

#[test]
fn scan_accepts_normal_relative_path() {
    with_cache(|engine| {
        let scan = engine
            .scan(Some("examples/fixtures/log-diagnosis"), None, None, None)
            .unwrap();
        assert!(scan["session_id"].is_string());
    });
}

#[test]
fn cache_rejects_traversal_in_env() {
    let _guard = test_lock::acquire();
    std::env::set_var("RLM_CACHE_DIR", "nested\\..\\escape");
    assert!(project::init_cache().is_err());
    std::env::remove_var("RLM_CACHE_DIR");
}

#[test]
fn cache_info_reports_layout() {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    let info = project::cache_info().unwrap();
    assert!(info["cache_dir"].is_string());
    assert!(info["subdirs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| { s.as_str() == Some("rlm-sessions") }));
    std::env::remove_var("RLM_CACHE_DIR");
}

#[test]
fn chunk_output_truncates_when_limit_env_set() {
    with_cache(|engine| {
        std::env::set_var("RLM_MAX_CHUNK_BYTES", "32");
        let big = "x".repeat(200);
        let scan = engine
            .scan(None, Some(&big), Some("big.txt"), None)
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        let chunk = engine.chunk(session_id, None, None, 0, 1, true).unwrap();
        let first = &chunk["chunks"][0];
        assert_eq!(first["truncated"], true);
        assert!(first["content"].as_str().unwrap().len() <= 32);
        std::env::remove_var("RLM_MAX_CHUNK_BYTES");
    });
}
