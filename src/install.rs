use crate::error::{Error, Result};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const SERVER_NAME: &str = "rlm-mcp";
const LEGACY_SERVER_NAMES: &[&str] = &["codebase-memory-rlm-mcp"];

pub fn configure_opencode(binary: &Path) -> Result<Vec<PathBuf>> {
    if let Ok(path) = std::env::var("OPENCODE_CONFIG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let paths = vec![PathBuf::from(trimmed)];
            write_opencode_config(&paths[0], binary)?;
            return Ok(paths);
        }
    }
    let config_dir = opencode_config_dir()?;
    let json = config_dir.join("opencode.json");
    let jsonc = config_dir.join("opencode.jsonc");
    let mut paths = Vec::new();

    if json.exists() {
        paths.push(json);
    }
    if jsonc.exists() {
        paths.push(jsonc);
    }
    if paths.is_empty() {
        paths.push(config_dir.join("opencode.json"));
    }

    for path in &paths {
        write_opencode_config(path, binary)?;
    }
    Ok(paths)
}

fn opencode_config_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("OPENCODE_CONFIG_DIR") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    dirs::home_dir()
        .map(|home| home.join(".config").join("opencode"))
        .ok_or_else(|| Error::Other("home directory not found".into()))
}

fn write_opencode_config(path: &Path, binary: &Path) -> Result<()> {
    let mut root: Map<String, Value> = if path.exists() {
        parse_json_config(&fs::read_to_string(path)?)?
    } else {
        Map::new()
    };
    let mcp = root
        .entry("mcp")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::Other("opencode mcp field is not an object".into()))?;

    for legacy in LEGACY_SERVER_NAMES {
        mcp.remove(*legacy);
    }
    mcp.insert(
        SERVER_NAME.into(),
        json!({
            "type": "local",
            "command": [binary.to_string_lossy()],
            "enabled": true,
            "timeout": 120000,
            "environment": {}
        }),
    );

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&root)? + "\n")?;
    Ok(())
}

fn parse_json_config(content: &str) -> Result<Map<String, Value>> {
    match serde_json::from_str::<Value>(content) {
        Ok(Value::Object(root)) => Ok(root),
        Ok(_) => Err(Error::Other("config root is not an object".into())),
        Err(first) => match serde_json::from_str::<Value>(&normalize_jsonc(content)) {
            Ok(Value::Object(root)) => Ok(root),
            Ok(_) => Err(Error::Other("config root is not an object".into())),
            Err(_) => Err(first.into()),
        },
    }
}

fn normalize_jsonc(content: &str) -> String {
    strip_trailing_json_commas(&strip_json_comments(content))
}

fn strip_json_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for next in chars.by_ref() {
                        if prev == '*' && next == '/' {
                            break;
                        }
                        prev = next;
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(ch);
    }
    out
}

fn strip_trailing_json_commas(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == ',' {
            let mut lookahead = chars.clone();
            while matches!(lookahead.peek(), Some(c) if c.is_whitespace()) {
                lookahead.next();
            }
            if matches!(lookahead.peek(), Some('}' | ']')) {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn writes_opencode_entry_and_removes_legacy_name() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("opencode.jsonc");
        fs::write(
            &config,
            r#"{
  // Existing JSONC remains readable.
  "mcp": {
    "codebase-memory-rlm-mcp": {"command": ["old.exe"],},
  },
}"#,
        )
        .unwrap();
        let binary = dir.path().join("rlm-mcp.exe");

        write_opencode_config(&config, &binary).unwrap();

        let parsed: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        assert!(parsed["mcp"]["rlm-mcp"].is_object());
        assert!(parsed["mcp"].get("codebase-memory-rlm-mcp").is_none());
        assert_eq!(
            parsed["mcp"]["rlm-mcp"]["command"][0].as_str(),
            Some(binary.to_string_lossy().as_ref())
        );
    }
}
