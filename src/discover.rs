use ignore::WalkBuilder;
use std::path::Path;

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".codebase-memory",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    ".cargo",
];

pub const SKIP_SUFFIXES: &[&str] = &[
    ".pyc", ".pyo", ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".lib", ".png", ".jpg", ".jpeg",
    ".gif", ".webp", ".ico", ".svg", ".mp3", ".mp4", ".wav", ".zip", ".tar", ".gz", ".zst",
    ".woff", ".woff2", ".ttf", ".eot",
];

pub const SKIP_FILENAMES: &[&str] = &["package-lock.json", "yarn.lock", "go.sum", "Cargo.lock"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Full,
}

pub fn language_for_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    Some(
        match ext {
            "rs" => "rust",
            "py" | "pyi" => "python",
            "js" | "mjs" | "cjs" => "javascript",
            "ts" | "mts" | "cts" => "typescript",
            "tsx" => "tsx",
            "jsx" => "jsx",
            "go" => "go",
            "java" => "java",
            "kt" => "kotlin",
            "c" | "h" => "c",
            "cpp" | "cc" | "cxx" | "hpp" => "cpp",
            "cs" => "csharp",
            "rb" => "ruby",
            "php" => "php",
            "swift" => "swift",
            "zig" => "zig",
            "lua" => "lua",
            "sh" | "bash" => "shell",
            "ps1" => "powershell",
            "sql" => "sql",
            "html" | "htm" => "html",
            "css" | "scss" => "css",
            "json" => "json",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            "md" => "markdown",
            "txt" | "log" | "csv" => "text",
            _ => return None,
        }
        .to_string(),
    )
}

/// Shared ignore-aware walker for RLM scan.
pub fn configure_walker(repo_path: &Path, _mode: IndexMode) -> WalkBuilder {
    let mut builder = WalkBuilder::new(repo_path);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .add_custom_ignore_filename(".cbmignore")
        .filter_entry(move |entry| {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if SKIP_DIRS.contains(&name) {
                        return false;
                    }
                }
            }
            true
        });
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_language() {
        assert_eq!(language_for_path(Path::new("foo.rs")), Some("rust".into()));
        assert_eq!(language_for_path(Path::new("foo.txt")), Some("text".into()));
    }
}
