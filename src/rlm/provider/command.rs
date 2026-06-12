use super::{ProviderResult, SubModelProvider};
use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::process::{Command, Stdio};

pub struct CommandProvider {
    program: String,
    args: Vec<String>,
}

impl CommandProvider {
    pub fn from_env() -> Result<Self> {
        let program = std::env::var("RLM_PROVIDER_COMMAND").map_err(|_| {
            Error::InvalidArgument(
                "command provider requires RLM_PROVIDER_COMMAND (executable path)".into(),
            )
        })?;
        let args = std::env::var("RLM_PROVIDER_ARGS")
            .ok()
            .map(parse_args)
            .unwrap_or_default();
        Ok(Self { program, args })
    }

    #[cfg(test)]
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

fn parse_args(raw: String) -> Vec<String> {
    if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&raw) {
        return parsed;
    }
    raw.split_whitespace().map(str::to_string).collect()
}

impl SubModelProvider for CommandProvider {
    fn name(&self) -> &str {
        "command"
    }

    fn invoke(&self, prompt: &str, context: &str) -> Result<ProviderResult> {
        let payload = json!({ "prompt": prompt, "context": context });
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .env("RLM_SUB_PROMPT", prompt)
            .env("RLM_SUB_CONTEXT", context)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::Other(format!(
                    "failed to spawn RLM_PROVIDER_COMMAND '{}': {e}",
                    self.program
                ))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let body = payload.to_string();
            stdin.write_all(body.as_bytes()).map_err(Error::Io)?;
        }

        let output = child.wait_with_output().map_err(Error::Io)?;
        if !output.status.success() {
            return Err(Error::Other(format!(
                "command provider exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        parse_command_output(&stdout)
    }
}

fn parse_command_output(stdout: &str) -> Result<ProviderResult> {
    if let Ok(parsed) = serde_json::from_str::<Value>(stdout) {
        if let Some(output) = parsed.get("output").and_then(|v| v.as_str()) {
            let structured = parsed
                .get("structured")
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            let input_tokens_est = parsed
                .get("input_tokens_est")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let output_tokens_est = parsed
                .get("output_tokens_est")
                .and_then(|v| v.as_u64())
                .unwrap_or(output.len() as u64 / 4) as usize;
            return Ok(ProviderResult {
                output: output.to_string(),
                structured,
                input_tokens_est,
                output_tokens_est,
                provider: "command".into(),
                usage: None,
                cost_usd_est: None,
            });
        }
    }

    Ok(ProviderResult {
        output: stdout.to_string(),
        structured: json!({
            "summary": stdout,
            "findings": [{ "summary": stdout, "confidence": 0.7 }]
        }),
        input_tokens_est: stdout.len() / 4,
        output_tokens_est: stdout.len() / 4,
        provider: "command".into(),
        usage: None,
        cost_usd_est: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_provider_parses_plain_stdout() {
        let provider = if cfg!(windows) {
            CommandProvider::new("cmd.exe", vec!["/C".into(), "echo".into(), "cmd-ok".into()])
        } else {
            CommandProvider::new("/bin/sh", vec!["-c".into(), "echo cmd-ok".into()])
        };
        let result = provider.invoke("task", "ctx").unwrap();
        assert!(result.output.contains("cmd-ok"));
        assert_eq!(result.provider, "command");
    }

    #[test]
    fn parse_args_accepts_json_array() {
        let args = parse_args(r#"["/C","echo","hi"]"#.into());
        assert_eq!(args, vec!["/C", "echo", "hi"]);
    }
}
