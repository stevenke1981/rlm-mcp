use rlm_mcp::rlm::RlmEngine;
use rlm_mcp::test_lock;
use tempfile::TempDir;

fn with_cache<F: FnOnce(&RlmEngine)>(f: F) {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    std::env::set_var("RLM_CACHE_DIR", cache.path());
    std::env::remove_var("RLM_ALLOW_REPL_EXEC");
    std::env::remove_var("RLM_REPL_COMMAND");
    std::env::remove_var("RLM_REPL_BACKEND");
    f(&RlmEngine::new());
    std::env::remove_var("RLM_CACHE_DIR");
    std::env::remove_var("RLM_ALLOW_REPL_EXEC");
    std::env::remove_var("RLM_REPL_COMMAND");
    std::env::remove_var("RLM_REPL_BACKEND");
}

#[test]
fn repl_info_lists_safe_builtin_default() {
    with_cache(|engine| {
        let info = engine.repl_info();
        assert_eq!(info["active_backend"].as_str().unwrap(), "safe_builtin");
        assert!(!info["repl_exec_enabled"].as_bool().unwrap());
        let ids: Vec<_> = info["backends"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["id"].as_str())
            .collect();
        assert!(ids.contains(&"safe_builtin"));
        assert!(ids.contains(&"command"));
    });
}

#[test]
fn repl_execute_rejects_without_opt_in() {
    with_cache(|engine| {
        let scan = engine
            .scan(None, Some("hello repl"), Some("repl.txt"), None)
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        let err = engine
            .repl_execute(session_id, "echo hi", None, Some("command"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("RLM_ALLOW_REPL_EXEC"));
    });
}

#[test]
fn env_info_includes_repl_section() {
    with_cache(|engine| {
        let scan = engine
            .scan(None, Some("ctx"), Some("ctx.txt"), None)
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        let env = engine.env_info(session_id).unwrap();
        assert!(env.get("repl").is_some());
        assert_eq!(
            env["repl"]["active_backend"].as_str().unwrap(),
            "safe_builtin"
        );
    });
}

#[cfg(unix)]
#[test]
fn repl_command_echoes_stdin_when_opted_in() {
    with_cache(|engine| {
        std::env::set_var("RLM_ALLOW_REPL_EXEC", "1");
        std::env::set_var("RLM_REPL_COMMAND", "cat");
        let scan = engine
            .scan(None, Some("needle"), Some("repl.txt"), None)
            .unwrap();
        let session_id = scan["session_id"].as_str().unwrap();
        let out = engine
            .repl_execute(session_id, "PING", None, Some("command"))
            .unwrap();
        assert_eq!(out["backend"].as_str().unwrap(), "command");
        assert!(out["content"].as_str().unwrap().contains("PING"));

        let traj = engine
            .trajectory_get(session_id, "json", true, &[])
            .unwrap();
        let events = traj["events"].as_array().unwrap();
        assert!(events
            .iter()
            .any(|e| e["event_type"].as_str() == Some("repl_exec")));
    });
}
