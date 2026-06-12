use rlm_mcp::rlm::PeekOptions;
use rlm_mcp::rlm::RlmEngine;
use rlm_mcp::test_lock;
use serde_json::json;
use tempfile::TempDir;

fn with_cache<F: FnOnce(&RlmEngine)>(f: F) {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    f(&RlmEngine::new());
    std::env::remove_var("RLM_CACHE_DIR");
}

#[test]
fn scan_peek_chunk_map_reduce_loop() {
    with_cache(|engine| {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("app.log"),
            "INFO start\nERROR disk full\nWARN retry\nERROR timeout\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("readme.md"), "# Demo\nno errors here\n").unwrap();

        let scan = engine
            .scan(
                Some(dir.path().to_string_lossy().as_ref()),
                None,
                None,
                None,
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();

        let env = engine.env_info(session_id).unwrap();
        assert_eq!(env["chunk_count"].as_u64().unwrap(), 2);

        let peek = engine
            .peek(
                session_id,
                PeekOptions {
                    query: Some("ERROR"),
                    limit: 10,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(peek["total_match_lines"].as_u64().unwrap(), 2);
        let chunk_id = peek["matches"][0]["chunk_id"].as_str().unwrap().to_string();

        let plan = engine
            .map_plan(session_id, Some(std::slice::from_ref(&chunk_id)), None, 1)
            .unwrap();
        assert_eq!(plan["total_chunks"].as_u64().unwrap(), 1);

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
        assert_eq!(chunk["chunks"].as_array().unwrap().len(), 1);

        let merged = engine
            .reduce_merge(&[json!({
                "batch_id": "batch-0",
                "findings": [{
                    "summary": "two ERROR lines in app.log",
                    "chunk_ids": [chunk_id],
                    "paths": ["app.log"]
                }],
                "unresolved": []
            })])
            .unwrap();
        assert_eq!(merged["finding_count"].as_u64().unwrap(), 1);
        assert!(!merged["needs_recursion"].as_bool().unwrap());
    });
}

#[test]
fn text_scan_and_slice() {
    with_cache(|engine| {
        let scan = engine
            .scan(
                None,
                Some("alpha\nbeta\ngamma\n"),
                Some("vars/prompt.txt"),
                Some("prompt"),
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        assert_eq!(scan["source_kind"].as_str().unwrap(), "text");

        let env = engine.env_info(session_id).unwrap();
        let chunk_id = env["files"][0]["chunk_ids"][0].as_str().unwrap();

        let slice = engine.slice(session_id, chunk_id, 2, 2).unwrap();
        assert_eq!(slice["content"].as_str().unwrap(), "beta");
    });
}

#[test]
fn peek_glob_and_regex() {
    with_cache(|engine| {
        let scan = engine
            .scan(
                None,
                Some("fn main() {}\nfn helper() {}\nstruct Data;\n"),
                Some("src/main.rs"),
                None,
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();

        let glob = engine
            .peek(
                session_id,
                PeekOptions {
                    glob: Some("*.rs"),
                    limit: 5,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(glob["returned"].as_u64().unwrap() >= 1);

        let regex = engine
            .peek(
                session_id,
                PeekOptions {
                    query: Some(r"fn \w+"),
                    regex: true,
                    limit: 5,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(regex["total_match_lines"].as_u64().unwrap(), 2);
    });
}

#[test]
fn recursive_subtask_with_mock_provider() {
    with_cache(|engine| {
        let scan = engine
            .scan(
                None,
                Some("fn main() {}\nfn helper() {}\n"),
                Some("src/lib.rs"),
                None,
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        let env = engine.env_info(session_id).unwrap();
        let chunk_id = env["files"][0]["chunk_ids"][0]
            .as_str()
            .unwrap()
            .to_string();

        let root = engine
            .task_create(
                session_id,
                "summarize functions",
                std::slice::from_ref(&chunk_id),
                None,
                "mock",
                None,
                None,
                true,
            )
            .unwrap();
        let root_id = root["root_id"].as_str().unwrap();

        let child = engine
            .task_create(
                session_id,
                "detail helper fn",
                std::slice::from_ref(&chunk_id),
                Some(root["task_id"].as_str().unwrap()),
                "mock",
                None,
                None,
                true,
            )
            .unwrap();
        assert_eq!(child["depth"].as_u64().unwrap(), 1);

        let reduced = engine.task_reduce(root_id).unwrap();
        assert_eq!(reduced["task_count"].as_u64().unwrap(), 2);
        assert_eq!(reduced["completed_count"].as_u64().unwrap(), 2);
        assert!(reduced["total_input_tokens_est"].as_u64().unwrap() > 0);
    });
}

#[test]
fn trajectory_records_full_loop() {
    with_cache(|engine| {
        let scan = engine
            .scan(
                None,
                Some("ERROR alpha\nINFO beta\nERROR gamma\n"),
                Some("app.log"),
                None,
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();

        engine
            .peek(
                session_id,
                PeekOptions {
                    query: Some("ERROR"),
                    limit: 5,
                    ..Default::default()
                },
            )
            .unwrap();

        let env = engine.env_info(session_id).unwrap();
        let chunk_id = env["files"][0]["chunk_ids"][0]
            .as_str()
            .unwrap()
            .to_string();

        engine
            .chunk(
                session_id,
                None,
                Some(std::slice::from_ref(&chunk_id)),
                0,
                1,
                true,
            )
            .unwrap();

        engine
            .task_create(
                session_id,
                "count errors",
                std::slice::from_ref(&chunk_id),
                None,
                "mock",
                None,
                None,
                true,
            )
            .unwrap();

        engine.trajectory_record_final(session_id, "two ERROR lines found", 2);

        let traj = engine
            .trajectory_get(session_id, "replay", true, &[])
            .unwrap();
        let summary = &traj["summary"];
        assert!(summary["event_count"].as_u64().unwrap() >= 5);
        assert!(summary["sub_call_count"].as_u64().unwrap() >= 1);
        assert!(summary["by_type"]["final_answer"].as_u64().unwrap() >= 1);

        let jsonl = engine
            .trajectory_get(session_id, "jsonl", true, &[])
            .unwrap();
        assert!(jsonl["jsonl"].as_str().unwrap().contains("scan"));
    });
}

#[test]
fn budget_limits_and_cancel() {
    with_cache(|engine| {
        use rlm_mcp::rlm::{BudgetMode, SessionBudget, TaskBudget};

        let scan = engine
            .scan(None, Some("line\n"), Some("x.txt"), None)
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();

        engine
            .budget_configure(SessionBudget {
                session_id: session_id.to_string(),
                mode: BudgetMode::FailFast,
                max_chunks_read: 1,
                max_sub_calls: 2,
                max_total_tokens_est: 10_000,
                max_wall_secs: 600,
                task_budget: TaskBudget::default(),
            })
            .unwrap();

        engine.chunk(session_id, None, None, 0, 1, true).unwrap();

        let blocked = engine.chunk(session_id, None, None, 0, 1, true);
        assert!(blocked.is_err());

        let status = engine.budget_status(session_id);
        assert_eq!(status["usage"]["chunks_read"].as_u64().unwrap(), 1);
        assert!(status["evaluation"]["allowed"].as_bool().unwrap());

        let env = engine.env_info(session_id).unwrap();
        let chunk_id = env["files"][0]["chunk_ids"][0]
            .as_str()
            .unwrap()
            .to_string();

        let root = engine
            .task_create(
                session_id,
                "work",
                std::slice::from_ref(&chunk_id),
                None,
                "mock",
                None,
                None,
                true,
            )
            .unwrap();

        let root_id = root["root_id"].as_str().unwrap();
        let cancelled = engine.task_cancel(root_id, "test stop").unwrap();
        assert!(cancelled["cancelled"].as_bool().unwrap());

        let err = engine
            .task_create(
                session_id,
                "after cancel",
                std::slice::from_ref(&chunk_id),
                Some(root["task_id"].as_str().unwrap()),
                "mock",
                None,
                None,
                true,
            )
            .unwrap_err();
        assert!(err.to_string().contains("cancelled"));
    });
}

#[test]
fn transform_and_artifact_round_trip() {
    with_cache(|engine| {
        let scan = engine
            .scan(
                None,
                Some("alpha\nbeta\nalpha\ngamma\n"),
                Some("lines.txt"),
                None,
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        let env = engine.env_info(session_id).unwrap();
        let chunk_id = env["files"][0]["chunk_ids"][0].as_str().unwrap();

        let transformed = engine
            .transform(
                session_id,
                "dedupe_lines",
                &json!({}),
                Some(chunk_id),
                None,
                None,
            )
            .unwrap();
        assert_eq!(transformed["content"], "alpha\nbeta\ngamma");

        engine
            .artifact_write(
                session_id,
                "deduped.txt",
                Some(transformed["content"].as_str().unwrap()),
                None,
            )
            .unwrap();

        let read = engine
            .artifact_read(session_id, "deduped.txt", None, None)
            .unwrap();
        assert_eq!(read["content"], "alpha\nbeta\ngamma");

        let traj = engine
            .trajectory_get(session_id, "json", true, &[])
            .unwrap();
        let types: Vec<_> = traj["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["event_type"].as_str())
            .collect();
        assert!(types.contains(&"transform"));
        assert!(types.contains(&"artifact_write"));
        assert!(types.contains(&"artifact_read"));
    });
}
