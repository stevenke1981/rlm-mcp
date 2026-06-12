mod command;
mod safe;

use crate::error::{Error, Result};
pub use command::CommandSandboxBackend;
pub use safe::SafeBuiltinBackend;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplBackendId {
    SafeBuiltin,
    Command,
    Python,
}

impl ReplBackendId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SafeBuiltin => "safe_builtin",
            Self::Command => "command",
            Self::Python => "python",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "safe" | "safe_builtin" | "builtin" => Some(Self::SafeBuiltin),
            "command" | "cmd" => Some(Self::Command),
            "python" | "python_repl" => Some(Self::Python),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReplCapabilities {
    pub executable: bool,
    pub network: bool,
    pub filesystem_read: bool,
    pub filesystem_write: bool,
    pub deterministic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxLimits {
    pub max_wall_secs: u64,
    pub max_memory_mb: u64,
    pub max_output_bytes: usize,
    pub max_input_bytes: usize,
    pub allow_network: bool,
}

impl SandboxLimits {
    pub fn from_env() -> Self {
        Self {
            max_wall_secs: env_u64("RLM_REPL_MAX_WALL_SECS", 30),
            max_memory_mb: env_u64("RLM_REPL_MAX_MEMORY_MB", 128),
            max_output_bytes: super::transform::max_output_bytes(),
            max_input_bytes: env_usize("RLM_REPL_MAX_INPUT_BYTES", 512 * 1024),
            allow_network: env_flag("RLM_ALLOW_NETWORK"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplBackendDescriptor {
    pub id: String,
    pub name: String,
    pub capabilities: ReplCapabilities,
    pub available: bool,
    pub requires_opt_in: bool,
    pub hint: String,
}

pub trait ReplBackend: Send + Sync {
    fn id(&self) -> ReplBackendId;
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> ReplCapabilities;
    fn descriptor(&self) -> ReplBackendDescriptor;
    fn execute_transform(&self, input: &str, operation: &str, params: &Value) -> Result<Value>;
    fn execute_code(&self, session_id: &str, code: &str, language: &str) -> Result<Value>;
}

pub fn repl_exec_enabled() -> bool {
    env_flag("RLM_ALLOW_REPL_EXEC")
}

pub fn configured_backend_id() -> ReplBackendId {
    std::env::var("RLM_REPL_BACKEND")
        .ok()
        .and_then(|v| ReplBackendId::parse(&v))
        .unwrap_or(ReplBackendId::SafeBuiltin)
}

pub fn safe_backend() -> SafeBuiltinBackend {
    SafeBuiltinBackend
}

pub fn list_backends() -> Value {
    let limits = SandboxLimits::from_env();
    let command = CommandSandboxBackend::new(limits.clone());
    let python = PythonReplBackend;
    let backends = [
        SafeBuiltinBackend.descriptor(),
        command.descriptor(),
        python.descriptor(),
    ];
    json!({
        "active_backend": configured_backend_id().as_str(),
        "repl_exec_enabled": repl_exec_enabled(),
        "limits": limits,
        "backends": backends,
        "opt_in": {
            "RLM_ALLOW_REPL_EXEC": "1 to enable command/python execution",
            "RLM_REPL_BACKEND": "safe_builtin (default), command, python",
            "RLM_REPL_COMMAND": "executable + args for command backend (stdin = code/input)",
        },
        "hint": "rlm_transform always uses safe_builtin; rlm_repl_execute is opt-in"
    })
}

pub fn session_fs_allowlist(session_id: &str) -> Vec<PathBuf> {
    let root = crate::project::default_cache_dir();
    vec![
        super::artifacts::artifacts_dir(session_id),
        root.join("rlm-sessions"),
        root.join("rlm-artifacts"),
    ]
}

pub fn path_within_allowlist(path: &Path, allowlist: &[PathBuf]) -> bool {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    allowlist.iter().any(|allowed| {
        let allowed_canon = allowed.canonicalize().unwrap_or_else(|_| allowed.clone());
        canon.starts_with(&allowed_canon)
    })
}

pub fn enforce_input_limit(input: &str, limits: &SandboxLimits) -> Result<()> {
    if input.len() > limits.max_input_bytes {
        return Err(Error::InvalidArgument(format!(
            "REPL input exceeds max {} bytes (got {})",
            limits.max_input_bytes,
            input.len()
        )));
    }
    Ok(())
}

pub fn truncate_output(output: &str, limits: &SandboxLimits) -> (String, bool) {
    if output.len() <= limits.max_output_bytes {
        return (output.to_string(), false);
    }
    let mut end = limits.max_output_bytes;
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    (output[..end].to_string(), true)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_flag(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

struct PythonReplBackend;

impl ReplBackend for PythonReplBackend {
    fn id(&self) -> ReplBackendId {
        ReplBackendId::Python
    }

    fn name(&self) -> &'static str {
        "python_repl"
    }

    fn capabilities(&self) -> ReplCapabilities {
        ReplCapabilities {
            executable: true,
            network: false,
            filesystem_read: true,
            filesystem_write: false,
            deterministic: false,
        }
    }

    fn descriptor(&self) -> ReplBackendDescriptor {
        ReplBackendDescriptor {
            id: self.id().as_str().into(),
            name: self.name().into(),
            capabilities: self.capabilities(),
            available: repl_exec_enabled(),
            requires_opt_in: true,
            hint: "Deferred: external Python REPL behind explicit opt-in".into(),
        }
    }

    fn execute_transform(&self, _input: &str, _operation: &str, _params: &Value) -> Result<Value> {
        Err(Error::InvalidArgument(
            "python backend does not support transform ops; use safe_builtin via rlm_transform"
                .into(),
        ))
    }

    fn execute_code(&self, _session_id: &str, _code: &str, _language: &str) -> Result<Value> {
        Err(Error::InvalidArgument(
            "python REPL backend is not implemented; use RLM_REPL_BACKEND=command with RLM_REPL_COMMAND".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_backend_is_default_without_env() {
        std::env::remove_var("RLM_REPL_BACKEND");
        assert_eq!(configured_backend_id(), ReplBackendId::SafeBuiltin);
    }

    #[test]
    fn list_backends_includes_safe_builtin() {
        let list = list_backends();
        let ids: Vec<_> = list["backends"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["id"].as_str())
            .collect();
        assert!(ids.contains(&"safe_builtin"));
    }
}
