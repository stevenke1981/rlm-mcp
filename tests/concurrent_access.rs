//! Concurrent session read + trajectory record tests for fine-grained locking.

use rlm_mcp::rlm::{PeekOptions, RlmEngine};
use rlm_mcp::test_lock;
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

fn with_cache<F: FnOnce(Arc<RlmEngine>)>(f: F) {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    f(Arc::new(RlmEngine::new()));
    std::env::remove_var("RLM_CACHE_DIR");
}

#[test]
fn concurrent_peek_and_chunk_on_shared_session() {
    with_cache(|engine| {
        let corpus = (0..40)
            .map(|i| format!("line-{i:02} ERROR marker filler text"))
            .collect::<Vec<_>>()
            .join("\n");
        let scan = engine
            .scan(None, Some(&corpus), Some("concurrent/log.txt"), None)
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap().to_string();

        let mut handles = Vec::new();
        for i in 0..6 {
            let eng = Arc::clone(&engine);
            let sid = session_id.clone();
            handles.push(thread::spawn(move || {
                if i % 2 == 0 {
                    eng.peek(
                        &sid,
                        PeekOptions {
                            query: Some("ERROR"),
                            limit: 5,
                            ..Default::default()
                        },
                    )
                    .unwrap()
                } else {
                    eng.chunk(&sid, None, None, 0, 2, true).unwrap()
                }
            }));
        }
        for h in handles {
            let out = h.join().unwrap();
            assert!(
                out.get("session_id").is_some()
                    || out.get("returned").is_some()
                    || out.get("chunks").is_some()
            );
        }

        let traj = engine
            .trajectory_get(&session_id, "json", true, &[])
            .unwrap();
        assert!(traj["summary"]["event_count"].as_u64().unwrap() >= 7); // scan + 6 ops
    });
}

#[test]
fn concurrent_trajectory_record_via_engine_ops() {
    with_cache(|engine| {
        let mut sessions = Vec::new();
        for i in 0..4 {
            let text = format!("session-{i}\nKEY=val-{i}\n");
            let scan = engine
                .scan(None, Some(&text), Some(&format!("s{i}.txt")), None)
                .unwrap();
            sessions.push(scan["session_id"].as_str().unwrap().to_string());
        }

        let mut handles = Vec::new();
        for sid in sessions.clone() {
            let eng = Arc::clone(&engine);
            handles.push(thread::spawn(move || {
                for _ in 0..10 {
                    eng.peek(
                        &sid,
                        PeekOptions {
                            query: Some("KEY"),
                            limit: 3,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        for sid in sessions {
            let traj = engine.trajectory_get(&sid, "json", true, &[]).unwrap();
            // 1 scan + 10 peeks
            assert_eq!(traj["summary"]["event_count"].as_u64().unwrap(), 11);
        }
    });
}
