use crate::error::{Error, Result};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

const CACHE_SUBDIRS: &[&str] = &[
    "rlm-sessions",
    "rlm-artifacts",
    "rlm-trajectories",
    "rlm-tasks",
    "rlm-map-plans",
    "rlm-budgets",
];

static DEFAULT_CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RLM_CACHE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return resolve_env_cache_root(trimmed).unwrap_or_else(|e| {
                tracing::warn!("RLM_CACHE_DIR invalid: {e}");
                fallback_cache_dir()
            });
        }
    }
    pinned_default_cache().unwrap_or_else(|e| {
        tracing::warn!("default cache init failed: {e}");
        fallback_cache_dir()
    })
}

/// Validate cache root, create layout, and pin default (non-env) cache for process lifetime.
pub fn init_cache() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("RLM_CACHE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return resolve_env_cache_root(trimmed);
        }
    }
    pinned_default_cache()
}

pub fn cache_info() -> Result<serde_json::Value> {
    let root = init_cache()?;
    Ok(serde_json::json!({
        "cache_dir": root.to_string_lossy(),
        "subdirs": CACHE_SUBDIRS,
        "env": {
            "RLM_CACHE_DIR": std::env::var("RLM_CACHE_DIR").ok(),
            "RLM_ALLOW_SYSTEM_TEMP": std::env::var("RLM_ALLOW_SYSTEM_TEMP").ok(),
        },
        "hint": "Dedicated cache under user data dir by default; never use bare system temp"
    }))
}

fn pinned_default_cache() -> Result<PathBuf> {
    if let Some(root) = DEFAULT_CACHE_ROOT.get() {
        return Ok(root.clone());
    }
    let root = prepare_cache_root(&fallback_cache_dir())?;
    let _ = DEFAULT_CACHE_ROOT.set(root.clone());
    Ok(root)
}

fn resolve_env_cache_root(path_str: &str) -> Result<PathBuf> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidArgument("cache path required".into()));
    }
    reject_path_traversal(trimmed)?;
    prepare_cache_root(&PathBuf::from(trimmed))
}

fn fallback_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rlm-mcp")
}

fn prepare_cache_root(root: &Path) -> Result<PathBuf> {
    validate_cache_root(root)?;
    std::fs::create_dir_all(root).map_err(|e| {
        Error::Other(format!(
            "failed to create cache dir {}: {e}",
            root.display()
        ))
    })?;
    set_private_dir(root)?;
    create_cache_layout(root)?;
    Ok(root.to_path_buf())
}

fn validate_cache_root(path: &Path) -> Result<()> {
    if is_unsafe_bare_system_temp(path) {
        return Err(Error::InvalidArgument(
            "refusing bare system temp as cache; use a dedicated subdir (e.g. .../rlm-mcp) or set RLM_ALLOW_SYSTEM_TEMP=1".into(),
        ));
    }
    Ok(())
}

fn is_unsafe_bare_system_temp(path: &Path) -> bool {
    if std::env::var("RLM_ALLOW_SYSTEM_TEMP")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        return false;
    }
    let temp = std::env::temp_dir();
    let path_canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let temp_canon = temp.canonicalize().unwrap_or(temp);
    path_canon == temp_canon
}

fn reject_path_traversal(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.split(['/', '\\']).any(|segment| segment == "..")
        || Path::new(trimmed)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(Error::InvalidArgument(
            "cache path must not contain '..' segments".into(),
        ));
    }
    Ok(())
}

fn create_cache_layout(root: &Path) -> Result<()> {
    for sub in CACHE_SUBDIRS {
        let dir = root.join(sub);
        std::fs::create_dir_all(&dir).map_err(|e| {
            Error::Other(format!(
                "failed to create cache subdir {}: {e}",
                dir.display()
            ))
        })?;
        set_private_dir(&dir)?;
    }
    Ok(())
}

fn set_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;
    use tempfile::TempDir;

    #[test]
    fn rejects_empty_cache_env() {
        let _guard = test_lock::acquire();
        std::env::set_var("RLM_CACHE_DIR", "   ");
        assert!(resolve_env_cache_root("   ").is_err());
        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn creates_expected_subdirs() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        let root = resolve_env_cache_root(cache.path().to_str().unwrap()).unwrap();
        for sub in CACHE_SUBDIRS {
            assert!(root.join(sub).is_dir(), "missing {sub}");
        }
    }

    #[test]
    fn rejects_bare_system_temp_without_opt_in() {
        let _guard = test_lock::acquire();
        let temp = std::env::temp_dir();
        std::env::remove_var("RLM_ALLOW_SYSTEM_TEMP");
        assert!(resolve_env_cache_root(temp.to_str().unwrap()).is_err());
    }
}
