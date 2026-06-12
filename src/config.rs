use std::path::PathBuf;

/// Resolve codebase-memory-mcp launch command.
pub fn resolve_cbm_binary() -> Vec<String> {
    if let Ok(env) = std::env::var("CBM_BINARY") {
        return vec![env];
    }

    if let Ok(env_cmd) = std::env::var("CBM_COMMAND") {
        return env_cmd.split_whitespace().map(str::to_string).collect();
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut candidates: Vec<PathBuf> = Vec::new();

    if cfg!(windows) {
        candidates.extend([
            PathBuf::from(r"D:\cbm-mcp\target\release\codebase-memory-mcp.exe"),
            home.join(".config")
                .join("codebase-memory-mcp")
                .join("bin")
                .join("codebase-memory-mcp.exe"),
            home.join(".config")
                .join("opencode-codebase-memory-mcp")
                .join("bin")
                .join("codebase-memory-mcp.exe"),
        ]);
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local)
                    .join("Programs")
                    .join("codebase-memory-mcp")
                    .join("codebase-memory-mcp.exe"),
            );
        }
    } else {
        candidates.extend([
            home.join(".local").join("bin").join("codebase-memory-mcp"),
            home.join(".config")
                .join("codebase-memory-mcp")
                .join("bin")
                .join("codebase-memory-mcp"),
        ]);
    }

    for path in candidates {
        if path.is_file() {
            return vec![path.to_string_lossy().to_string()];
        }
    }

    vec!["codebase-memory-mcp".into()]
}

pub fn default_project() -> Option<String> {
    std::env::var("CBM_PROJECT").ok()
}