use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn mcp_manifest_does_not_point_agents_at_target_release_binaries() {
    let manifest_path = root().join("packaging/mcp/manifest.json");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).expect("read manifest"))
            .expect("parse manifest");

    let install = &manifest["install"];
    let install_json = serde_json::to_string(install).expect("serialize install section");
    assert!(
        !install_json.contains("target/release") && !install_json.contains("target\\\\release"),
        "manifest install hints must point agents at release installers, not local build outputs"
    );
    assert_eq!(
        install["recommended_command"].as_str(),
        Some("./install.ps1")
    );
    assert!(
        install["windows_installer_url"]
            .as_str()
            .is_some_and(|url| url.contains("/packaging/windows/install.ps1")),
        "manifest should expose the no-compile Windows release installer URL"
    );
}

#[test]
fn windows_release_installer_has_locked_binary_fallback() {
    let script = fs::read_to_string(root().join("packaging/windows/install.ps1"))
        .expect("read windows installer");
    assert!(
        script.contains("Install-BinaryWithLockedFallback"),
        "Windows release installer should recover when the stable binary is locked"
    );
    assert!(
        script.contains("rlm-mcp-$VersionNoV.exe"),
        "locked-binary fallback should install a versioned side-by-side exe"
    );
}

#[test]
fn release_installers_can_resolve_latest_without_github_api() {
    let windows = fs::read_to_string(root().join("packaging/windows/install.ps1"))
        .expect("read windows installer");
    assert!(
        windows.contains("Resolve-LatestVersion")
            && windows.contains("releases/latest")
            && windows.contains("GITHUB_TOKEN")
            && windows.contains("GH_TOKEN"),
        "Windows installer should support authenticated API lookup and a public redirect fallback"
    );

    for relative_path in ["packaging/linux/install.sh", "packaging/macos/install.sh"] {
        let script = fs::read_to_string(root().join(relative_path))
            .unwrap_or_else(|_| panic!("read {relative_path}"));
        assert!(
            script.contains("releases/latest")
                && script.contains("url_effective")
                && script.contains("GITHUB_TOKEN")
                && script.contains("GH_TOKEN"),
            "{relative_path} should support authenticated API lookup and a public redirect fallback"
        );
    }
}
