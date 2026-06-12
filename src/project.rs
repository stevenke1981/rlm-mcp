use std::path::PathBuf;

/// Normalize project name; indexes use `cbm+` prefix (legacy `cbrlm+` accepted).
pub fn normalize_project_name(name: &str) -> String {
    if name.starts_with("cbm+") || name.starts_with("cbrlm+") {
        name.to_string()
    } else {
        format!("cbm+{name}")
    }
}

pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RLM_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("CBRLM_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("codebase-memory-rlm-mcp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_cbm_prefix() {
        assert_eq!(normalize_project_name("my-app"), "cbm+my-app");
        assert_eq!(normalize_project_name("cbm+my-app"), "cbm+my-app");
        assert_eq!(normalize_project_name("cbrlm+legacy"), "cbrlm+legacy");
    }
}