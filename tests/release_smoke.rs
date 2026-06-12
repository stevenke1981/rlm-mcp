use std::path::PathBuf;
use std::process::Command;

fn release_binary() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        root.join("target/release/rlm-mcp.exe"),
        root.join("target/release/rlm-mcp"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

#[test]
fn release_binary_runs_workflow_json() {
    let Some(bin) = release_binary() else {
        eprintln!("skip: release binary not built; run cargo build --release");
        return;
    };
    let output = Command::new(bin)
        .args(["workflow", "--phase", "overview", "--json"])
        .output()
        .expect("spawn release rlm-mcp");
    assert!(
        output.status.success(),
        "release workflow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("phase") || stdout.contains("overview"));
}