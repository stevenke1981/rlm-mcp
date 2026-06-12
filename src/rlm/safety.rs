use crate::error::{Error, Result};
use std::path::{Component, Path, PathBuf};

const DEFAULT_MAX_CHUNK_OUTPUT_BYTES: usize = 256 * 1024;

pub fn max_chunk_output_bytes() -> usize {
    std::env::var("RLM_MAX_CHUNK_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_CHUNK_OUTPUT_BYTES)
}

/// Reject paths that contain `..` components before filesystem resolution.
pub fn reject_path_traversal(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidArgument("path required".into()));
    }
    if trimmed.split(['/', '\\']).any(|segment| segment == "..")
        || Path::new(trimmed)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Error::InvalidArgument(
            "path must not contain '..' segments".into(),
        ));
    }
    Ok(())
}

pub fn resolve_scan_path(path: &str) -> Result<PathBuf> {
    reject_path_traversal(path)?;
    let root = Path::new(path)
        .canonicalize()
        .map_err(|e| Error::InvalidArgument(format!("invalid scan path: {e}")))?;
    if !root.exists() {
        return Err(Error::InvalidArgument(format!("path not found: {path}")));
    }
    Ok(root)
}

pub fn truncate_text(content: &str, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content.to_string(), false);
    }
    let truncated: String = content.chars().take(max_bytes).collect();
    (truncated, true)
}

pub fn truncate_chunk_content(content: &str) -> (String, bool) {
    truncate_text(content, max_chunk_output_bytes())
}

/// Heuristic: NUL bytes or a high ratio of non-text control chars.
pub fn is_probably_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if bytes.contains(&0) {
        return true;
    }
    let non_text = bytes
        .iter()
        .filter(|&&b| b < 0x09 || (b > 0x0d && b < 0x20))
        .count();
    non_text * 10 > bytes.len()
}

pub fn default_secret_patterns() -> Vec<String> {
    vec![
        "sk-".into(),
        "Bearer ".into(),
        "api_key=".into(),
        "api-key=".into(),
        "password=".into(),
        "secret=".into(),
        "Authorization:".into(),
    ]
}

pub fn redact_secrets(text: &str, extra_patterns: &[String]) -> String {
    let mut out = text.to_string();
    for pat in default_secret_patterns()
        .iter()
        .chain(extra_patterns.iter())
    {
        if out.contains(pat.as_str()) {
            out = out.replace(pat.as_str(), "[REDACTED]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_dir_segments() {
        assert!(reject_path_traversal("..\\windows").is_err());
        assert!(reject_path_traversal("logs\\..\\secret").is_err());
        assert!(reject_path_traversal("examples/fixtures").is_ok());
    }

    #[test]
    fn detects_binary_nul() {
        assert!(is_probably_binary(b"hello\0world"));
        assert!(!is_probably_binary(b"hello\nworld"));
    }

    #[test]
    fn truncates_chunk_content() {
        let (out, truncated) = truncate_text("abcdef", 3);
        assert!(truncated);
        assert_eq!(out, "abc");
    }

    #[test]
    fn redacts_common_secret_markers() {
        let out = redact_secrets("token=Bearer sk-abc123 password=secret", &[]);
        assert!(!out.contains("Bearer "));
        assert!(!out.contains("sk-"));
        assert!(!out.contains("password="));
    }
}
