use rlm_mcp::test_lock;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn bin() -> String {
    for key in ["CARGO_BIN_EXE_rlm_mcp", "CARGO_BIN_EXE_rlm-mcp"] {
        if let Ok(path) = std::env::var(key) {
            return path;
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let profile = std::env::var("CARGO_PROFILE").unwrap_or_else(|_| "debug".into());
    let candidates = [
        root.join("target").join(&profile).join("rlm-mcp.exe"),
        root.join("target").join(&profile).join("rlm-mcp"),
    ];
    for path in candidates {
        if path.exists() {
            return path.to_string_lossy().into_owned();
        }
    }
    panic!("rlm-mcp binary not found; run cargo build first");
}

fn run_json(args: &[&str]) -> Value {
    let output = Command::new(bin())
        .args(args)
        .output()
        .expect("spawn rlm-mcp");
    assert!(
        output.status.success(),
        "command failed: {:?}\nstderr={}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    serde_json::from_str(stdout.trim()).expect("parseable json stdout")
}

#[test]
fn cli_workflow_json_contract() {
    let value = run_json(&["workflow", "--phase", "overview", "--json"]);
    assert!(value.get("phase").is_some() || value.get("overview").is_some());
}

#[test]
fn cli_version_flag_prints_package_version() {
    let output = Command::new(bin())
        .arg("--version")
        .output()
        .expect("spawn rlm-mcp --version");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn cli_reduce_schema_json_contract() {
    let value = run_json(&["reduce-schema", "--json"]);
    assert!(value.get("worker_schema").is_some() || value.get("checklist").is_some());
}

#[test]
fn cli_tools_reference_json_contract() {
    let value = run_json(&["tools-reference", "--json"]);
    assert_eq!(value["tool_count"].as_u64().unwrap(), 33);
    assert!(value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| { t["name"].as_str() == Some("rlm_scan") }));
}

#[test]
fn cli_benchmark_list_json_contract() {
    let value = run_json(&["benchmark", "list", "--json"]);
    assert!(value["suites"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| { s["id"].as_str() == Some("sniah") }));
}

#[test]
fn cli_scan_peek_json_contract() {
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();
    let output = Command::new(bin())
        .env("RLM_CACHE_DIR", cache.path())
        .args([
            "scan",
            "--content",
            "line one\nERROR found\nline three\n",
            "--virtual-path",
            "contract/cli.log",
            "--json",
        ])
        .output()
        .expect("spawn scan");
    assert!(output.status.success());
    let scan: Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap();
    let session_id = scan["session_id"].as_str().unwrap();

    let peek = Command::new(bin())
        .env("RLM_CACHE_DIR", cache.path())
        .args([
            "peek",
            "--session-id",
            session_id,
            "--query",
            "ERROR",
            "--json",
        ])
        .output()
        .expect("spawn peek");
    assert!(peek.status.success());
    let peek_json: Value =
        serde_json::from_str(String::from_utf8(peek.stdout).unwrap().trim()).unwrap();
    assert!(peek_json["total_match_lines"].as_u64().unwrap() >= 1);
}
