use rlm_mcp::rlm::{PeekOptions, RlmEngine};
use rlm_mcp::rlm::SessionStore;
use rlm_mcp::test_lock;
use std::collections::HashMap;
use tempfile::TempDir;

fn with_cache<F: FnOnce()>(f: F) {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    f();
    std::env::remove_var("RLM_CACHE_DIR");
}

#[test]
fn hydrate_loads_session_from_disk_for_second_process() {
    with_cache(|| {
        let id = {
            let mut store = SessionStore::new();
            let session = store
                .create_from_text("alpha\nbeta\n", "cross.txt", HashMap::new())
                .unwrap();
            session.id.clone()
        };

        let mut early = SessionStore::new();
        early.hydrate(&id).unwrap();
        assert_eq!(early.get(&id).unwrap().chunks.len(), 1);
    });
}

#[test]
fn delete_blocks_subsequent_hydrate() {
    with_cache(|| {
        let mut early = SessionStore::new();
        let id = {
            let mut writer = SessionStore::new();
            writer
                .create_from_text("x\n", "t.txt", HashMap::new())
                .unwrap()
                .id
        };

        let mut deleter = SessionStore::new();
        deleter.delete(&id).unwrap();

        assert!(early.hydrate(&id).is_err());
    });
}

#[test]
fn import_export_round_trip() {
    with_cache(|| {
        let mut store = SessionStore::new();
        let original = store
            .create_from_text("needle\n", "n.txt", HashMap::new())
            .unwrap();
        let exported = store.export(&original.id).unwrap();

        store.delete(&original.id).unwrap();

        let imported = store.import_session(exported, true).unwrap();
        assert_eq!(imported.id, original.id);
        assert_eq!(imported.chunks[0].content, "needle");
    });
}

#[test]
fn multi_worker_map_batches_fixture() {
    with_cache(|| {
        let engine = RlmEngine::new();
        let scan = engine
            .scan(
                None,
                Some("file-a line\nfile-b line\nfile-c line\n"),
                Some("docs/a.txt"),
                None,
            )
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();

        let plan = engine
            .map_plan(session_id, None, None, 2)
            .unwrap();
        assert_eq!(plan["total_chunks"].as_u64().unwrap(), 1);
        assert!(!plan["batches"].as_array().unwrap().is_empty());

        let peek = engine
            .peek(
                session_id,
                PeekOptions {
                    query: Some("line"),
                    limit: 5,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(peek["returned"].as_u64().unwrap() >= 1);
    });
}