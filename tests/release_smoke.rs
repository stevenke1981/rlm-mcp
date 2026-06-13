use rlm_mcp::test_lock;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn release_binary() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        root.join("target/release/rlm-mcp.exe"),
        root.join("target/release/rlm-mcp"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn bin_string() -> Option<String> {
    release_binary().map(|p| p.to_string_lossy().into_owned())
}

fn run_json(bin: &str, args: &[&str]) -> Value {
    let output = Command::new(bin)
        .args(args)
        .output()
        .expect("spawn release rlm-mcp");
    assert!(
        output.status.success(),
        "command failed: {:?}\nstderr={}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).expect("json stdout")
}

fn sha256_hex(path: &PathBuf) -> String {
    let bytes = std::fs::read(path).expect("read binary");
    format!("{:x}", Sha256::digest(bytes))
}

fn write_mcp_frame(writer: &mut impl Write, body: &str) {
    writeln!(writer, "{body}").unwrap();
    writer.flush().unwrap();
}

fn read_mcp_frame(reader: &mut impl BufRead) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read JSON-RPC line");
    line.trim_end().to_string()
}

#[test]
fn release_binary_runs_workflow_json() {
    let Some(bin) = bin_string() else {
        eprintln!("skip: release binary not built; run cargo build --release");
        return;
    };
    let value = run_json(&bin, &["workflow", "--phase", "overview", "--json"]);
    assert!(value.get("phase").is_some() || value.get("overview").is_some());
}

#[test]
fn release_binary_has_checksum_and_minimum_size() {
    let Some(path) = release_binary() else {
        eprintln!("skip: release binary not built");
        return;
    };
    let meta = std::fs::metadata(&path).expect("metadata");
    assert!(
        meta.len() > 200_000,
        "release binary suspiciously small: {} bytes",
        meta.len()
    );
    let hash = sha256_hex(&path);
    assert_eq!(hash.len(), 64);
    assert_eq!(hash, sha256_hex(&path), "checksum must be stable");
}

#[test]
fn release_binary_scan_peek_chunk_cli() {
    let Some(bin) = bin_string() else {
        eprintln!("skip: release binary not built");
        return;
    };
    let _guard = test_lock::acquire();
    let cache = TempDir::new().unwrap();

    let scan = Command::new(&bin)
        .env("RLM_CACHE_DIR", cache.path())
        .args([
            "scan",
            "--content",
            "alpha\nNEEDLE=42\nomega\n",
            "--virtual-path",
            "release/smoke.txt",
            "--json",
        ])
        .output()
        .expect("scan");
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let scan_json: Value =
        serde_json::from_str(String::from_utf8(scan.stdout).unwrap().trim()).unwrap();
    let session_id = scan_json["session_id"].as_str().unwrap();

    let peek = Command::new(&bin)
        .env("RLM_CACHE_DIR", cache.path())
        .args([
            "peek",
            "--session-id",
            session_id,
            "--query",
            "NEEDLE",
            "--json",
        ])
        .output()
        .expect("peek");
    assert!(peek.status.success());
    let peek_json: Value =
        serde_json::from_str(String::from_utf8(peek.stdout).unwrap().trim()).unwrap();
    assert!(peek_json["total_match_lines"].as_u64().unwrap() >= 1);

    let chunk = Command::new(&bin)
        .env("RLM_CACHE_DIR", cache.path())
        .args([
            "chunk",
            "--session-id",
            session_id,
            "--offset",
            "0",
            "--limit",
            "1",
            "--json",
        ])
        .output()
        .expect("chunk");
    assert!(chunk.status.success());
    let chunk_json: Value =
        serde_json::from_str(String::from_utf8(chunk.stdout).unwrap().trim()).unwrap();
    assert_eq!(chunk_json["chunks"].as_array().unwrap().len(), 1);
}

#[test]
fn release_binary_mcp_stdio_initialize_and_tools_list() {
    let Some(bin) = bin_string() else {
        eprintln!("skip: release binary not built");
        return;
    };

    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp server");

    let stdin = child.stdin.as_mut().expect("stdin");
    write_mcp_frame(
        stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"release-smoke","version":"1"}}}"#,
    );

    let stdout = child.stdout.take().expect("stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let init_body = read_mcp_frame(&mut reader);
    let init: Value = serde_json::from_str(&init_body).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "rlm-mcp");

    write_mcp_frame(
        stdin,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    write_mcp_frame(
        stdin,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    );

    let list_body = read_mcp_frame(&mut reader);
    let list: Value = serde_json::from_str(&list_body).unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 33);

    let _ = child.kill();
    let _ = child.wait();
}
