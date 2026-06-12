use std::path::PathBuf;

pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RLM_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rlm-mcp")
}
