use super::{
    enforce_input_limit, path_within_allowlist, repl_exec_enabled, session_fs_allowlist,
    truncate_output, ReplBackend, ReplBackendDescriptor, ReplBackendId, ReplCapabilities,
    SandboxLimits,
};
use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CommandSandboxBackend {
    limits: SandboxLimits,
}

impl CommandSandboxBackend {
    pub fn new(limits: SandboxLimits) -> Self {
        Self { limits }
    }

    pub fn descriptor_static(limits: &SandboxLimits) -> ReplBackendDescriptor {
        let cmd = std::env::var("RLM_REPL_COMMAND").ok();
        ReplBackendDescriptor {
            id: ReplBackendId::Command.as_str().into(),
            name: "command".into(),
            capabilities: ReplCapabilities {
                executable: true,
                network: limits.allow_network,
                filesystem_read: true,
                filesystem_write: false,
                deterministic: false,
            },
            available: repl_exec_enabled() && cmd.is_some(),
            requires_opt_in: true,
            hint: format!(
                "Runs RLM_REPL_COMMAND with code on stdin; wall {}s, output {} bytes",
                limits.max_wall_secs, limits.max_output_bytes
            ),
        }
    }

    fn require_opt_in(&self) -> Result<()> {
        if !repl_exec_enabled() {
            return Err(Error::InvalidArgument(
                "command REPL requires RLM_ALLOW_REPL_EXEC=1".into(),
            ));
        }
        Ok(())
    }

    fn parse_command() -> Result<(String, Vec<String>)> {
        let raw = std::env::var("RLM_REPL_COMMAND").map_err(|_| {
            Error::InvalidArgument(
                "command REPL requires RLM_REPL_COMMAND (executable and optional args)".into(),
            )
        })?;
        let parts: Vec<&str> = raw.split_whitespace().collect();
        if parts.is_empty() {
            return Err(Error::InvalidArgument("RLM_REPL_COMMAND is empty".into()));
        }
        Ok((
            parts[0].to_string(),
            parts[1..].iter().map(|s| (*s).to_string()).collect(),
        ))
    }

    fn working_dir(session_id: &str) -> Result<PathBuf> {
        let dir = super::super::artifacts::artifacts_dir(session_id);
        std::fs::create_dir_all(&dir)?;
        let allowlist = session_fs_allowlist(session_id);
        if !path_within_allowlist(&dir, &allowlist) {
            return Err(Error::InvalidArgument(
                "REPL working directory outside session allowlist".into(),
            ));
        }
        Ok(dir)
    }

    pub fn run_with_input(&self, session_id: &str, input: &str) -> Result<(String, u64, bool)> {
        self.require_opt_in()?;
        enforce_input_limit(input, &self.limits)?;
        if self.limits.allow_network {
            return Err(Error::InvalidArgument(
                "network-enabled REPL is not implemented; keep RLM_ALLOW_NETWORK unset".into(),
            ));
        }

        let (program, args) = Self::parse_command()?;
        let cwd = Self::working_dir(session_id)?;
        let started = Instant::now();
        let timeout = Duration::from_secs(self.limits.max_wall_secs);

        let mut child = Command::new(&program)
            .args(&args)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Other(format!("failed to spawn RLM_REPL_COMMAND: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .map_err(|e| Error::Other(format!("REPL stdin write failed: {e}")))?;
        }

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();
                    if let Some(mut out) = child.stdout.take() {
                        std::io::Read::read_to_end(&mut out, &mut stdout).map_err(Error::Io)?;
                    }
                    if let Some(mut err) = child.stderr.take() {
                        std::io::Read::read_to_end(&mut err, &mut stderr).map_err(Error::Io)?;
                    }
                    let stdout = String::from_utf8_lossy(&stdout).into_owned();
                    let stderr = String::from_utf8_lossy(&stderr).into_owned();
                    if !status.success() {
                        return Err(Error::Other(format!(
                            "REPL command exited {}: {}",
                            status,
                            stderr.trim()
                        )));
                    }
                    let (content, truncated) = truncate_output(&stdout, &self.limits);
                    return Ok((content, started.elapsed().as_millis() as u64, truncated));
                }
                Ok(None) => {
                    if started.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(Error::Other(format!(
                            "REPL command exceeded wall limit of {}s",
                            self.limits.max_wall_secs
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => return Err(Error::Io(e)),
            }
        }
    }
}

impl ReplBackend for CommandSandboxBackend {
    fn id(&self) -> ReplBackendId {
        ReplBackendId::Command
    }

    fn name(&self) -> &'static str {
        "command"
    }

    fn capabilities(&self) -> ReplCapabilities {
        Self::descriptor_static(&self.limits).capabilities
    }

    fn descriptor(&self) -> ReplBackendDescriptor {
        let cmd = std::env::var("RLM_REPL_COMMAND").ok();
        ReplBackendDescriptor {
            id: self.id().as_str().into(),
            name: self.name().into(),
            capabilities: self.capabilities(),
            available: super::repl_exec_enabled() && cmd.is_some(),
            requires_opt_in: true,
            hint: format!(
                "Runs RLM_REPL_COMMAND with code on stdin; wall {}s, output {} bytes",
                self.limits.max_wall_secs, self.limits.max_output_bytes
            ),
        }
    }

    fn execute_transform(&self, _input: &str, _operation: &str, _params: &Value) -> Result<Value> {
        Err(Error::InvalidArgument(
            "command backend does not support transform ops; use safe_builtin via rlm_transform"
                .into(),
        ))
    }

    fn execute_code(&self, session_id: &str, code: &str, language: &str) -> Result<Value> {
        let (content, wall_ms, truncated) = self.run_with_input(session_id, code)?;
        Ok(json!({
            "backend": self.name(),
            "language": language,
            "content": content,
            "output_chars": content.len(),
            "truncated": truncated,
            "wall_ms": wall_ms,
            "limits": self.limits,
            "audit": {
                "backend": self.name(),
                "input_bytes": code.len(),
                "output_bytes": content.len(),
                "wall_ms": wall_ms,
                "truncated": truncated,
            }
        }))
    }
}
