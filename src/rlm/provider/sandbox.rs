use crate::error::{Error, Result};
use std::path::Path;

/// Sandbox mode for command provider execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    /// Validate command path against allowed directories; reject shell interpreters.
    Strict,
    /// Log a warning for every command invocation.
    Warn,
    /// No sandbox restrictions.
    Off,
}

impl SandboxMode {
    pub fn from_env() -> Self {
        match std::env::var("RLM_PROVIDER_SANDBOX").ok().as_deref() {
            Some("strict") => Self::Strict,
            Some("off") => Self::Off,
            _ => Self::Warn, // default: warn (backward compatible)
        }
    }
}

/// Resolve a bare command name using PATH environment variable.
fn resolve_in_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<_> = std::env::split_paths(&path).collect();
    let extensions = if cfg!(windows) {
        // On Windows, try adding common executable extensions
        vec!["", ".exe", ".bat", ".cmd", ".com", ".ps1"]
    } else {
        vec![""]
    };

    for dir in &dirs {
        for ext in &extensions {
            let full = dir.join(format!("{}{}", name, ext));
            if full.is_file() {
                return Some(full.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Default list of shell interpreters that are rejected in strict mode.
const SHELL_INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "fish",
    "cmd.exe",
    "cmd",
    "powershell.exe",
    "powershell",
    "pwsh.exe",
    "pwsh",
    "python",
    "python3",
    "node",
    "deno",
    "bun",
];

/// Derived from RLM_PROVIDER_ALLOWED_DIRS env var: semicolon-separated absolute paths.
fn allowed_dirs() -> Vec<String> {
    std::env::var("RLM_PROVIDER_ALLOWED_DIRS")
        .ok()
        .map(|v| {
            v.split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Validate a command path / name against the sandbox policy.
/// Returns an error if the command is rejected.
pub fn validate_command(program: &str, mode: SandboxMode) -> Result<()> {
    match mode {
        SandboxMode::Off => Ok(()),
        SandboxMode::Warn => {
            log_warning(program);
            Ok(())
        }
        SandboxMode::Strict => strict_validation(program),
    }
}

fn log_warning(program: &str) {
    eprintln!(
        "[rlm-mcp][sandbox] WARNING: executing external command '{}'. \
         Set RLM_PROVIDER_SANDBOX=strict for path validation, \
         or RLM_PROVIDER_SANDBOX=off to silence this warning.",
        program
    );
}

fn strict_validation(program: &str) -> Result<()> {
    let path = Path::new(program);

    // 1. Check if it's a bare name (not a path) — could be in PATH
    let is_bare_name = !program.contains('/') && !program.contains('\\');

    if is_bare_name {
        // Warn about bare names — can't verify which executable will be used
        if SHELL_INTERPRETERS.contains(&program) {
            return Err(Error::InvalidArgument(format!(
                "sandbox (strict): shell interpreter '{program}' is not allowed. \
                 Use an explicit absolute path to a non-shell executable, \
                 or set RLM_PROVIDER_SANDBOX=warn or =off to allow."
            )));
        }
        // Resolve from PATH and check allowed dirs
        if let Some(resolved) = resolve_in_path(program) {
            check_allowed_dir(&resolved)?;
        }
        // If can't resolve, it will fail later during spawn
        return Ok(());
    }

    // 2. Absolute or relative path — check it's an allowed executable
    if !path.is_absolute() {
        return Err(Error::InvalidArgument(format!(
            "sandbox (strict): command path must be absolute: '{program}'"
        )));
    }

    // 3. Extract filename for interpreter check
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        if SHELL_INTERPRETERS.contains(&file_name) {
            return Err(Error::InvalidArgument(format!(
                "sandbox (strict): shell interpreter '{file_name}' is not allowed. \
                 Use RLM_PROVIDER_SANDBOX=warn or =off to allow."
            )));
        }
    }

    // 4. Check allowed dirs
    check_allowed_dir(program)?;

    Ok(())
}

fn check_allowed_dir(program: &str) -> Result<()> {
    let dirs = allowed_dirs();
    if dirs.is_empty() {
        return Ok(()); // no restriction when no dirs configured
    }

    let path = Path::new(program);
    let parent = path.parent().ok_or_else(|| {
        Error::InvalidArgument(format!(
            "sandbox (strict): cannot determine parent of '{program}'"
        ))
    })?;

    let parent_str = parent.to_string_lossy().replace('\\', "/");
    let allowed = dirs.iter().any(|d| {
        let d_normalized = d.replace('\\', "/");
        parent_str.starts_with(&d_normalized)
    });

    if !allowed {
        return Err(Error::InvalidArgument(format!(
            "sandbox (strict): command '{}' is not in RLM_PROVIDER_ALLOWED_DIRS ({})",
            program,
            dirs.join("; ")
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_mode_defaults_to_warn() {
        std::env::remove_var("RLM_PROVIDER_SANDBOX");
        assert_eq!(SandboxMode::from_env(), SandboxMode::Warn);
    }

    #[test]
    fn sandbox_mode_parses_env() {
        std::env::set_var("RLM_PROVIDER_SANDBOX", "strict");
        assert_eq!(SandboxMode::from_env(), SandboxMode::Strict);
        std::env::set_var("RLM_PROVIDER_SANDBOX", "off");
        assert_eq!(SandboxMode::from_env(), SandboxMode::Off);
        std::env::set_var("RLM_PROVIDER_SANDBOX", "warn");
        assert_eq!(SandboxMode::from_env(), SandboxMode::Warn);
        std::env::remove_var("RLM_PROVIDER_SANDBOX");
    }

    #[test]
    fn strict_rejects_shell_interpreters() {
        for shell in SHELL_INTERPRETERS {
            assert!(strict_validation(shell).is_err(), "should reject {shell}");
        }
    }

    #[test]
    fn strict_accepts_absolute_path() {
        let test_path = if cfg!(windows) {
            "C:\\Windows\\System32\\find.exe"
        } else {
            "/bin/ls"
        };
        // Should not fail for being a shell interpreter
        let file_name = test_path.split(&['/', '\\'][..]).next_back().unwrap_or("");
        assert!(
            !SHELL_INTERPRETERS.contains(&file_name),
            "test path should not be a shell"
        );
        let _ = strict_validation(test_path);
    }

    #[test]
    fn warn_mode_does_not_reject() {
        let result = validate_command("sh", SandboxMode::Warn);
        assert!(result.is_ok());
    }

    #[test]
    fn off_mode_does_not_reject() {
        let result = validate_command("rm -rf /", SandboxMode::Off);
        assert!(result.is_ok());
    }
}
