use super::{ReplBackend, ReplBackendDescriptor, ReplBackendId, ReplCapabilities, SandboxLimits};
use crate::error::Result;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub struct SafeBuiltinBackend;

impl SafeBuiltinBackend {
    pub fn descriptor_static() -> ReplBackendDescriptor {
        ReplBackendDescriptor {
            id: ReplBackendId::SafeBuiltin.as_str().into(),
            name: "safe_builtin".into(),
            capabilities: ReplCapabilities {
                executable: false,
                network: false,
                filesystem_read: false,
                filesystem_write: false,
                deterministic: true,
            },
            available: true,
            requires_opt_in: false,
            hint: "Default for rlm_transform; deterministic string ops only".into(),
        }
    }
}

impl ReplBackend for SafeBuiltinBackend {
    fn id(&self) -> ReplBackendId {
        ReplBackendId::SafeBuiltin
    }

    fn name(&self) -> &'static str {
        "safe_builtin"
    }

    fn capabilities(&self) -> ReplCapabilities {
        Self::descriptor_static().capabilities
    }

    fn descriptor(&self) -> ReplBackendDescriptor {
        ReplBackendDescriptor {
            id: self.id().as_str().into(),
            name: self.name().into(),
            capabilities: self.capabilities(),
            available: true,
            requires_opt_in: false,
            hint: "Default for rlm_transform; deterministic string ops only".into(),
        }
    }

    fn execute_transform(&self, input: &str, operation: &str, params: &Value) -> Result<Value> {
        let limits = SandboxLimits::from_env();
        super::enforce_input_limit(input, &limits)?;
        let mut out = super::super::transform::apply(input, operation, params)?;
        if let Some(content) = out.get("content").and_then(|v| v.as_str()) {
            let (truncated, was_truncated) = super::truncate_output(content, &limits);
            if was_truncated {
                out["content"] = Value::String(truncated.clone());
                out["truncated"] = Value::Bool(true);
                out["output_chars"] = Value::Number(truncated.len().into());
            }
        }
        out["backend"] = Value::String(self.name().into());
        Ok(out)
    }

    fn execute_code(&self, _session_id: &str, _code: &str, _language: &str) -> Result<Value> {
        Err(crate::error::Error::InvalidArgument(
            "safe_builtin backend cannot execute code; use rlm_transform or opt into command backend".into(),
        ))
    }
}
