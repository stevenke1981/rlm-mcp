//! OpenAI-compatible chat completions provider using **async** `reqwest`.
//!
//! Sync [`SubModelProvider::invoke`] bridges to the Tokio runtime (via
//! `block_in_place` when already inside a runtime, otherwise a dedicated
//! multi-thread runtime). Requests honor MCP cancel (`cancel::is_cancelled`)
//! and `RLM_OPENAI_TIMEOUT_SECS` / `RLM_PROVIDER_MAX_WALL_SECS`.

use super::{ProviderResult, ProviderUsage, SubModelProvider};
use crate::error::{Error, Result};
use crate::rlm::cancel;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub struct OpenAiCompatibleProvider {
    api_key: String,
    base_url: String,
    model: String,
    prompt_cost_per_1k: Option<f64>,
    completion_cost_per_1k: Option<f64>,
}

fn http_timeout() -> Duration {
    if let Ok(v) = std::env::var("RLM_OPENAI_TIMEOUT_SECS") {
        if let Ok(secs) = v.parse::<u64>() {
            if secs == 0 {
                return Duration::from_secs(120);
            }
            return Duration::from_secs(secs);
        }
    }
    match std::env::var("RLM_PROVIDER_MAX_WALL_SECS") {
        Ok(v) => {
            let secs: u64 = v.parse().unwrap_or(120);
            if secs == 0 {
                Duration::from_secs(300)
            } else {
                Duration::from_secs(secs.min(600))
            }
        }
        Err(_) => Duration::from_secs(120),
    }
}

fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(http_timeout())
            .connect_timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(4)
            .user_agent(format!("rlm-mcp/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build reqwest client")
    })
}

fn block_on<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => {
            static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
            let rt = RT.get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .thread_name("rlm-http")
                    .build()
                    .expect("failed to build rlm-http runtime")
            });
            rt.block_on(fut)
        }
    }
}

async fn wait_for_cancel() {
    loop {
        if cancel::is_cancelled() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

impl OpenAiCompatibleProvider {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("RLM_OPENAI_API_KEY").map_err(|_| {
            Error::InvalidArgument(
                "openai provider requires RLM_OPENAI_API_KEY (never stored in sessions)".into(),
            )
        })?;
        let base_url = std::env::var("RLM_OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".into())
            .trim_end_matches('/')
            .to_string();
        let model = std::env::var("RLM_OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        let prompt_cost_per_1k = std::env::var("RLM_OPENAI_PROMPT_COST_PER_1K")
            .ok()
            .and_then(|v| v.parse().ok());
        let completion_cost_per_1k = std::env::var("RLM_OPENAI_COMPLETION_COST_PER_1K")
            .ok()
            .and_then(|v| v.parse().ok());
        Ok(Self {
            api_key,
            base_url,
            model,
            prompt_cost_per_1k,
            completion_cost_per_1k,
        })
    }

    pub fn parse_chat_response(body: &Value) -> Result<ProviderResult> {
        let output = body
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Other("openai response missing choices[0].message.content".into())
            })?
            .to_string();

        let usage = body.get("usage");
        let prompt_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64());
        let completion_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64());
        let total_tokens = usage
            .and_then(|u| u.get("total_tokens"))
            .and_then(|v| v.as_u64());

        let input_tokens_est = prompt_tokens.unwrap_or((output.len() / 4) as u64) as usize;
        let output_tokens_est = completion_tokens.unwrap_or((output.len() / 4) as u64) as usize;

        let structured = json!({
            "summary": output,
            "findings": [{ "summary": output, "confidence": 0.8 }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": total_tokens
            }
        });

        Ok(ProviderResult {
            output: output.clone(),
            structured,
            input_tokens_est,
            output_tokens_est,
            provider: "openai".into(),
            usage: Some(ProviderUsage {
                prompt_tokens: prompt_tokens.map(|v| v as u32),
                completion_tokens: completion_tokens.map(|v| v as u32),
                total_tokens: total_tokens.map(|v| v as u32),
            }),
            cost_usd_est: None,
        })
    }

    fn estimate_cost(
        &self,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> Option<f64> {
        let prompt_rate = self.prompt_cost_per_1k?;
        let completion_rate = self.completion_cost_per_1k?;
        let prompt = prompt_tokens.unwrap_or(0) as f64;
        let completion = completion_tokens.unwrap_or(0) as f64;
        Some((prompt / 1000.0) * prompt_rate + (completion / 1000.0) * completion_rate)
    }

    /// Async chat completion with cancel + timeout.
    pub async fn invoke_async(&self, prompt: &str, context: &str) -> Result<ProviderResult> {
        if cancel::is_cancelled() {
            return Err(Error::Cancelled("openai request cancelled".into()));
        }

        let url = format!("{}/chat/completions", self.base_url);
        let request_body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are an RLM sub-task worker. Analyze only the provided context snippet and return a concise structured answer."
                },
                {
                    "role": "user",
                    "content": format!("Task:\n{prompt}\n\nContext snippet:\n{context}")
                }
            ]
        });

        let client = shared_client();
        let timeout = http_timeout();
        let send_fut = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send();

        let response = tokio::select! {
            biased;
            _ = wait_for_cancel() => {
                return Err(Error::Cancelled("openai request cancelled".into()));
            }
            timed = tokio::time::timeout(timeout, send_fut) => {
                match timed {
                    Ok(Ok(resp)) => resp,
                    Ok(Err(e)) => {
                        return Err(Error::Other(format!("openai request failed: {e}")));
                    }
                    Err(_) => {
                        return Err(Error::Other(format!(
                            "openai request timeout after {}s",
                            timeout.as_secs()
                        )));
                    }
                }
            }
        };

        if cancel::is_cancelled() {
            return Err(Error::Cancelled("openai request cancelled".into()));
        }

        let status = response.status();
        let body_text = tokio::select! {
            biased;
            _ = wait_for_cancel() => {
                return Err(Error::Cancelled("openai request cancelled".into()));
            }
            text = response.text() => {
                text.map_err(|e| Error::Other(format!(
                    "openai response body read failed (status {status}): {e}"
                )))?
            }
        };

        let body: Value = serde_json::from_str(&body_text).map_err(|e| {
            Error::Other(format!(
                "openai response parse failed (status {status}): {e}"
            ))
        })?;

        if !status.is_success() {
            let message = body
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown provider error");
            return Err(Error::Other(format!(
                "openai provider error (status {status}): {message}"
            )));
        }

        let mut result = Self::parse_chat_response(&body)?;
        result.cost_usd_est = self.estimate_cost(
            result.usage.as_ref().and_then(|u| u.prompt_tokens),
            result.usage.as_ref().and_then(|u| u.completion_tokens),
        );
        Ok(result)
    }
}

impl SubModelProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn invoke(&self, prompt: &str, context: &str) -> Result<ProviderResult> {
        block_on(self.invoke_async(prompt, context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rlm::cancel::CancelGuard;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn parses_openai_chat_response() {
        let body = json!({
            "choices": [{ "message": { "content": "needle found" } }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 4, "total_tokens": 16 }
        });
        let result = OpenAiCompatibleProvider::parse_chat_response(&body).unwrap();
        assert_eq!(result.output, "needle found");
        assert_eq!(result.input_tokens_est, 12);
        assert_eq!(result.usage.as_ref().unwrap().total_tokens, Some(16));
    }

    #[test]
    fn estimates_cost_when_rates_configured() {
        let provider = OpenAiCompatibleProvider {
            api_key: "k".into(),
            base_url: "https://example.com/v1".into(),
            model: "m".into(),
            prompt_cost_per_1k: Some(0.5),
            completion_cost_per_1k: Some(1.5),
        };
        let cost = provider.estimate_cost(Some(1000), Some(500)).unwrap();
        assert!((cost - 1.25).abs() < 0.001);
    }

    #[test]
    fn invoke_async_respects_cancel_before_request() {
        let token = CancellationToken::new();
        let _guard = CancelGuard::install(token.clone());
        token.cancel();
        let provider = OpenAiCompatibleProvider {
            api_key: "k".into(),
            base_url: "https://example.invalid/v1".into(),
            model: "m".into(),
            prompt_cost_per_1k: None,
            completion_cost_per_1k: None,
        };
        let err = provider.invoke("p", "c").unwrap_err();
        assert!(
            matches!(err, Error::Cancelled(_)) || err.to_string().contains("cancelled"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn http_timeout_defaults_positive() {
        std::env::remove_var("RLM_OPENAI_TIMEOUT_SECS");
        std::env::remove_var("RLM_PROVIDER_MAX_WALL_SECS");
        assert!(http_timeout().as_secs() >= 30);
    }
}
