use crate::config::resolve_cbm_binary;
use crate::error::{Error, Result};
use crate::mcp::transport::{read_message, write_message};
use serde_json::{json, Value};
use std::io::BufReader;
use std::process::{Command, Stdio};

/// MCP client that spawns codebase-memory-mcp per tool call (stdio).
pub struct CbmClient {
    command: Vec<String>,
}

impl CbmClient {
    pub fn new() -> Self {
        Self {
            command: resolve_cbm_binary(),
        }
    }

    pub fn with_command(command: Vec<String>) -> Self {
        Self { command }
    }

    pub fn command(&self) -> &[String] {
        &self.command
    }

    pub fn call_tool(&self, name: &str, arguments: &Value) -> Result<Value> {
        if self.command.is_empty() {
            return Err(Error::Cbm("codebase-memory-mcp binary not configured".into()));
        }

        let mut child = Command::new(&self.command[0])
            .args(&self.command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                Error::Cbm(format!(
                    "failed to spawn {}: {e}",
                    self.command[0]
                ))
            })?;

        let mut stdin = child.stdin.take().ok_or_else(|| Error::Cbm("stdin unavailable".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| Error::Cbm("stdout unavailable".into()))?;
        let mut reader = BufReader::new(stdout);

        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "codebase-memory-rlm-mcp", "version": env!("CARGO_PKG_VERSION") }
            }
        });
        write_message(&mut stdin, &init_req.to_string())?;
        let init_resp = read_message(&mut reader)?
            .ok_or_else(|| Error::Cbm("CBM closed before initialize response".into()))?;
        let _: Value = serde_json::from_str(&init_resp)
            .map_err(|e| Error::Cbm(format!("invalid initialize response: {e}")))?;

        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        write_message(&mut stdin, &initialized.to_string())?;

        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        write_message(&mut stdin, &call_req.to_string())?;
        drop(stdin);

        let call_resp = read_message(&mut reader)?
            .ok_or_else(|| Error::Cbm(format!("CBM closed before tools/call response for {name}")))?;
        let _ = child.wait();

        parse_tool_result(&call_resp)
    }

    pub fn search_graph(
        &self,
        project: &str,
        query: Option<&str>,
        name_pattern: Option<&str>,
        label: Option<&str>,
        limit: u64,
    ) -> Result<Value> {
        let mut args = json!({ "project": project, "limit": limit });
        if let Some(q) = query {
            args["query"] = json!(q);
        }
        if let Some(p) = name_pattern {
            args["name_pattern"] = json!(p);
        }
        if let Some(l) = label {
            args["label"] = json!(l);
        }
        self.call_tool("search_graph", &args)
    }

    pub fn search_code_files(
        &self,
        project: &str,
        pattern: &str,
        file_pattern: Option<&str>,
        limit: u64,
    ) -> Result<Value> {
        let mut args = json!({
            "project": project,
            "pattern": pattern,
            "mode": "files",
            "limit": limit
        });
        if let Some(fp) = file_pattern {
            args["file_pattern"] = json!(fp);
        }
        self.call_tool("search_code", &args)
    }

    pub fn get_code_snippet(&self, project: &str, qualified_name: &str) -> Result<Value> {
        self.call_tool(
            "get_code_snippet",
            &json!({ "project": project, "qualified_name": qualified_name }),
        )
    }

    pub fn trace_path(
        &self,
        project: &str,
        function_name: &str,
        direction: &str,
        depth: u64,
        mode: &str,
    ) -> Result<Value> {
        self.call_tool(
            "trace_path",
            &json!({
                "project": project,
                "function_name": function_name,
                "direction": direction,
                "depth": depth,
                "mode": mode
            }),
        )
    }

    pub fn get_architecture(&self, project: &str) -> Result<Value> {
        self.call_tool("get_architecture", &json!({ "project": project }))
    }

    pub fn index_status(&self, project: &str) -> Result<Value> {
        self.call_tool("index_status", &json!({ "project": project }))
    }

    pub fn detect_changes(&self, project: &str, scope: Option<&str>) -> Result<Value> {
        let mut args = json!({ "project": project });
        if let Some(s) = scope {
            args["scope"] = json!(s);
        }
        self.call_tool("detect_changes", &args)
    }
}

impl Default for CbmClient {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_tool_result(raw: &str) -> Result<Value> {
    let envelope: Value = serde_json::from_str(raw)?;
    if let Some(err) = envelope.get("error") {
        return Err(Error::Cbm(err.to_string()));
    }
    let result = envelope
        .get("result")
        .ok_or_else(|| Error::Cbm("missing result in CBM response".into()))?;

    if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
        let text = extract_text(result);
        return Err(Error::Cbm(text));
    }

    let text = extract_text(result);
    if text.trim().is_empty() {
        return Ok(result.clone());
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => Ok(value),
        Err(_) => Ok(json!({ "text": text })),
    }
}

fn extract_text(result: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        for item in content {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        result.to_string()
    } else {
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_mcp_content() {
        let raw = r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"ok\":true}"}],"isError":false}}"#;
        let value = parse_tool_result(raw).unwrap();
        assert_eq!(value.get("ok").and_then(|v| v.as_bool()), Some(true));
    }
}