mod command;
mod openai;
mod retry;

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use command::CommandProvider;
pub use openai::OpenAiCompatibleProvider;
pub use retry::RetryProvider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResult {
    pub output: String,
    pub structured: Value,
    pub input_tokens_est: usize,
    pub output_tokens_est: usize,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ProviderUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd_est: Option<f64>,
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
            usage: None,
            cost_usd_est: None,
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
            usage: None,
            cost_usd_est: None,
        })
    }
}

pub fn network_allowed() -> bool {
    matches!(
        std::env::var("RLM_ALLOW_NETWORK").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

fn require_network(provider: &str) -> Result<()> {
    if network_allowed() {
        return Ok(());
    }
    Err(Error::InvalidArgument(format!(
        "provider '{provider}' requires network opt-in: set RLM_ALLOW_NETWORK=1"
    )))
}

pub fn available_providers() -> &'static [&'static str] {
    &["mock", "dry-run", "command", "openai"]
}

pub fn resolve_provider(name: &str) -> Result<Box<dyn SubModelProvider>> {
    let base: Box<dyn SubModelProvider> = match name {
        "mock" => Box::new(MockProvider),
        "dry-run" | "dry_run" => Box::new(DryRunProvider),
        "command" | "local" => Box::new(CommandProvider::from_env()?),
        "openai" | "openai-compatible" | "openai_compatible" => {
            require_network(name)?;
            Box::new(OpenAiCompatibleProvider::from_env()?)
        }
        "none" | "external" => {
            return Err(Error::InvalidArgument(
                "provider 'none' means agent-managed only; use mock, dry-run, command, or openai"
                    .into(),
            ));
        }
        other => {
            return Err(Error::InvalidArgument(format!(
                "unknown provider: {other} (available: {})",
                available_providers().join(", ")
            )));
        }
    };
    Ok(Box::new(RetryProvider::wrap(base)))
}

fn is_sensitive_key(key: &str) -> bool {
    const ALLOW: &[&str] = &[
        "prompt_tokens",
        "completion_tokens",
        "total_tokens",
        "input_tokens_est",
        "output_tokens_est",
    ];
    let key_lower = key.to_lowercase();
    if ALLOW.contains(&key_lower.as_str()) {
        return false;
    }
    key_lower.contains("api_key")
        || key_lower.contains("authorization")
        || key_lower.contains("secret")
        || key_lower.contains("password")
        || (key_lower.contains("token") && key_lower != "total_tokens")
}

pub fn sanitize_structured(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if is_sensitive_key(k) {
                    continue;
                }
                out.insert(k.clone(), sanitize_structured(v));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sanitize_structured).collect()),
        other => other.clone(),
    }
}

pub fn sanitize_result(mut result: ProviderResult) -> ProviderResult {
    result.structured = sanitize_structured(&result.structured);
    result
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

    #[test]
    fn openai_requires_network_opt_in() {
        std::env::remove_var("RLM_ALLOW_NETWORK");
        std::env::set_var("RLM_OPENAI_API_KEY", "test-key");
        match resolve_provider("openai") {
            Err(err) => assert!(err.to_string().contains("RLM_ALLOW_NETWORK")),
            Ok(_) => panic!("expected network opt-in error"),
        }
        std::env::remove_var("RLM_OPENAI_API_KEY");
    }

    #[test]
    fn sanitize_strips_api_key_fields() {
        let raw = json!({
            "summary": "ok",
            "api_key": "sk-secret",
            "nested": { "authorization": "Bearer x" },
            "usage": { "prompt_tokens": 3 }
        });
        let clean = sanitize_structured(&raw);
        assert!(clean.get("api_key").is_none());
        assert!(clean["nested"].get("authorization").is_none());
        assert_eq!(clean["usage"]["prompt_tokens"].as_u64().unwrap(), 3);
    }
}
