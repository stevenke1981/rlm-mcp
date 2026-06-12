use super::{ProviderResult, SubModelProvider};
use crate::error::{Error, Result};
use std::thread;
use std::time::Duration;

pub struct RetryProvider {
    inner: Box<dyn SubModelProvider>,
    max_retries: u32,
    base_delay_ms: u64,
}

impl RetryProvider {
    pub fn wrap(inner: Box<dyn SubModelProvider>) -> Self {
        let max_retries = std::env::var("RLM_PROVIDER_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        let base_delay_ms = std::env::var("RLM_PROVIDER_RETRY_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        Self {
            inner,
            max_retries,
            base_delay_ms,
        }
    }
}

impl SubModelProvider for RetryProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn invoke(&self, prompt: &str, context: &str) -> Result<ProviderResult> {
        let mut attempt = 0u32;
        loop {
            match self.inner.invoke(prompt, context) {
                Ok(result) => return Ok(result),
                Err(err) if attempt < self.max_retries && is_retryable(&err) => {
                    let delay = self.base_delay_ms.saturating_mul(1u64 << attempt.min(6));
                    thread::sleep(Duration::from_millis(delay));
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }
}

fn is_retryable(err: &Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("rate limit")
        || msg.contains("timeout")
        || msg.contains("temporarily unavailable")
        || msg.contains("503")
        || msg.contains("429")
        || matches!(err, Error::Io(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct FlakyProvider {
        remaining_failures: std::sync::Mutex<u32>,
    }

    impl SubModelProvider for FlakyProvider {
        fn name(&self) -> &str {
            "flaky"
        }

        fn invoke(&self, _prompt: &str, _context: &str) -> Result<ProviderResult> {
            let mut guard = self.remaining_failures.lock().unwrap();
            if *guard > 0 {
                *guard -= 1;
                return Err(Error::Other("503 temporarily unavailable".into()));
            }
            Ok(ProviderResult {
                output: "ok".into(),
                structured: json!({}),
                input_tokens_est: 1,
                output_tokens_est: 1,
                provider: "flaky".into(),
                usage: None,
                cost_usd_est: None,
            })
        }
    }

    #[test]
    fn retries_retryable_errors() {
        let inner: Box<dyn SubModelProvider> = Box::new(FlakyProvider {
            remaining_failures: std::sync::Mutex::new(1),
        });
        let provider = RetryProvider {
            inner,
            max_retries: 2,
            base_delay_ms: 1,
        };
        let result = provider.invoke("p", "c").unwrap();
        assert_eq!(result.output, "ok");
    }
}
