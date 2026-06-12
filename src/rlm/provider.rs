use crate::error::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResult {
    pub output: String,
    pub structured: Value,
    pub input_tokens_est: usize,
    pub output_tokens_est: usize,
    pub provider: String,
}

pub trait SubModelProvider: Send + Sync {
    fn name(&self) -> &str;
    fn invoke(&self, prompt: &str, context: &str) -> Result<ProviderResult>;
}

pub struct MockProvider;

impl SubModelProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn invoke(&self, prompt: &str, context: &str) -> Result<ProviderResult> {
        let line_count = context.lines().count();
        let keyword = prompt
            .split_whitespace()
            .find(|w| w.len() > 3)
            .unwrap_or("task");
        let output = format!(
            "mock analysis for '{keyword}': {line_count} context lines, {} bytes",
            context.len()
        );
        let structured = json!({
            "summary": output,
            "keyword": keyword,
            "context_lines": line_count,
            "context_bytes": context.len(),
            "findings": [{
                "summary": output,
                "confidence": 0.85
            }]
        });
        Ok(ProviderResult {
            output: output.clone(),
            structured,
            input_tokens_est: (prompt.len() + context.len()) / 4,
            output_tokens_est: output.len() / 4,
            provider: self.name().into(),
        })
    }
}

pub struct DryRunProvider;

impl SubModelProvider for DryRunProvider {
    fn name(&self) -> &str {
        "dry-run"
    }

    fn invoke(&self, prompt: &str, context: &str) -> Result<ProviderResult> {
        let structured = json!({
            "dry_run": true,
            "prompt_preview": &prompt[..prompt.len().min(120)],
            "context_bytes": context.len(),
            "would_invoke": true
        });
        Ok(ProviderResult {
            output: "dry-run: no provider call made".into(),
            structured,
            input_tokens_est: 0,
            output_tokens_est: 0,
            provider: self.name().into(),
        })
    }
}

pub fn resolve_provider(name: &str) -> Result<Box<dyn SubModelProvider>> {
    match name {
        "mock" => Ok(Box::new(MockProvider)),
        "dry-run" | "dry_run" => Ok(Box::new(DryRunProvider)),
        "none" | "external" => Err(crate::error::Error::InvalidArgument(
            "provider 'none' means agent-managed only; use mock or dry-run for local execution"
                .into(),
        )),
        other => Err(crate::error::Error::InvalidArgument(format!(
            "unknown provider: {other} (available: mock, dry-run)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_provider_is_deterministic() {
        let p = MockProvider;
        let r1 = p
            .invoke("find ERROR patterns", "line1\nERROR foo\n")
            .unwrap();
        let r2 = p
            .invoke("find ERROR patterns", "line1\nERROR foo\n")
            .unwrap();
        assert_eq!(r1.output, r2.output);
        assert!(r1.structured["findings"].is_array());
    }

    #[test]
    fn dry_run_skips_tokens() {
        let p = DryRunProvider;
        let r = p.invoke("test", "context").unwrap();
        assert_eq!(r.input_tokens_est, 0);
        assert!(r.structured["dry_run"].as_bool().unwrap());
    }
}
