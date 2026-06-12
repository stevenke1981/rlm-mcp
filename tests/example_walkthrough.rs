//! Runs the documented log-diagnosis example fixture end-to-end.
use rlm_mcp::rlm::PeekOptions;
use rlm_mcp::rlm::RlmEngine;
use rlm_mcp::test_lock;
use serde_json::json;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/fixtures/log-diagnosis")
}

fn with_cache<F: FnOnce(&RlmEngine)>(f: F) {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    f(&RlmEngine::new());
    std::env::remove_var("RLM_CACHE_DIR");
}

#[test]
fn log_diagnosis_walkthrough_matches_docs() {
    with_cache(|engine| {
        let scan = engine
            .scan(
                Some(fixture_dir().to_string_lossy().as_ref()),
                None,
                None,
                None,
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        assert!(scan["chunk_count"].as_u64().unwrap() >= 2);

        let peek = engine
            .peek(
                session_id,
                PeekOptions {
                    query: Some("ERROR"),
                    limit: 20,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(peek["total_match_lines"].as_u64().unwrap(), 3);
        let chunk_id = peek["matches"][0]["chunk_id"].as_str().unwrap().to_string();

        let plan = engine
            .map_plan(session_id, Some(std::slice::from_ref(&chunk_id)), None, 1)
            .unwrap();
        assert_eq!(plan["batches"][0]["batch_id"].as_str().unwrap(), "batch-0");

        let chunk = engine
            .chunk(
                session_id,
                None,
                Some(std::slice::from_ref(&chunk_id)),
                0,
                1,
                true,
            )
            .unwrap();
        let content = chunk["chunks"][0]["content"].as_str().unwrap();
        assert!(content.contains("disk full"));

        let merged = engine
            .reduce_merge(&[json!({
                "batch_id": "batch-0",
                "worker_id": "worker-a",
                "findings": [{
                    "summary": "disk full cascade with write and flush errors",
                    "chunk_ids": [chunk_id],
                    "paths": ["app.log"],
                    "confidence": 0.92
                }],
                "unresolved": ["post-compaction recovery unclear"]
            })])
            .unwrap();
        assert!(merged["finding_count"].as_u64().unwrap() >= 1);

        let root = engine
            .task_create(
                session_id,
                "explain ERROR cascade",
                std::slice::from_ref(&chunk_id),
                None,
                "mock",
                None,
                None,
                true,
            )
            .unwrap();
        let reduced = engine
            .task_reduce(root["root_id"].as_str().unwrap())
            .unwrap();
        assert!(reduced["completed_count"].as_u64().unwrap() >= 1);

        engine.trajectory_record_final(session_id, "disk full caused write+flush errors", 3);
        let traj = engine
            .trajectory_get(session_id, "json", true, &[])
            .unwrap();
        assert!(traj["summary"]["event_count"].as_u64().unwrap() >= 6);
    });
}
