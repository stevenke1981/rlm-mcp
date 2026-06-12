use crate::error::{Error, Result};
use regex::Regex;
use serde_json::{json, Value};

const DEFAULT_MAX_OUTPUT_BYTES: usize = 256 * 1024;

pub fn max_output_bytes() -> usize {
    std::env::var("RLM_MAX_TRANSFORM_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
}

pub fn apply(content: &str, operation: &str, params: &Value) -> Result<Value> {
    let input_lines: Vec<&str> = content.lines().collect();
    let (output, meta) = match operation {
        "dedupe_lines" => {
            let mut seen = std::collections::HashSet::new();
            let lines: Vec<&str> = input_lines
                .iter()
                .copied()
                .filter(|line| seen.insert(*line))
                .collect();
            (
                lines.join("\n"),
                json!({ "deduped": input_lines.len().saturating_sub(lines.len()) }),
            )
        }
        "sort_lines" => {
            let reverse = params
                .get("reverse")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut lines: Vec<String> = input_lines.iter().map(|s| (*s).to_string()).collect();
            lines.sort();
            if reverse {
                lines.reverse();
            }
            (lines.join("\n"), json!({ "reverse": reverse }))
        }
        "filter_lines" => {
            let query = params
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::InvalidArgument("filter_lines requires query".into()))?;
            let regex = params
                .get("regex")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let case_sensitive = params
                .get("case_sensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let lines: Vec<&str> = if regex {
                let re = Regex::new(query)
                    .map_err(|e| Error::InvalidArgument(format!("invalid regex: {e}")))?;
                input_lines
                    .iter()
                    .copied()
                    .filter(|line| re.is_match(line))
                    .collect()
            } else if case_sensitive {
                input_lines
                    .iter()
                    .copied()
                    .filter(|line| line.contains(query))
                    .collect()
            } else {
                let q = query.to_lowercase();
                input_lines
                    .iter()
                    .copied()
                    .filter(|line| line.to_lowercase().contains(&q))
                    .collect()
            };
            (
                lines.join("\n"),
                json!({ "matched_lines": lines.len(), "query": query }),
            )
        }
        "head_lines" => {
            let n = params.get("n").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let lines: Vec<&str> = input_lines.iter().take(n).copied().collect();
            (lines.join("\n"), json!({ "n": n }))
        }
        "tail_lines" => {
            let n = params.get("n").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let start = input_lines.len().saturating_sub(n);
            let lines: Vec<&str> = input_lines.iter().skip(start).copied().collect();
            (lines.join("\n"), json!({ "n": n }))
        }
        "truncate_chars" => {
            let max = params.get("max").and_then(|v| v.as_u64()).unwrap_or(4096) as usize;
            let truncated = content.len() > max;
            let out = if truncated {
                content.chars().take(max).collect::<String>()
            } else {
                content.to_string()
            };
            (out, json!({ "max": max, "truncated": truncated }))
        }
        "add_line_numbers" => {
            let start = params.get("start").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let numbered: Vec<String> = input_lines
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{}: {}", start + i, line))
                .collect();
            (numbered.join("\n"), json!({ "start": start }))
        }
        "count_lines" => {
            let summary = format!("{} lines, {} chars", input_lines.len(), content.len());
            (
                summary,
                json!({
                    "line_count": input_lines.len(),
                    "char_count": content.len(),
                    "non_empty_lines": input_lines.iter().filter(|l| !l.is_empty()).count()
                }),
            )
        }
        "normalize_whitespace" => {
            let mut out_lines = Vec::new();
            let mut blank_run = 0usize;
            for line in &input_lines {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    blank_run += 1;
                    if blank_run <= 1 {
                        out_lines.push(String::new());
                    }
                } else {
                    blank_run = 0;
                    out_lines.push(trimmed.to_string());
                }
            }
            (out_lines.join("\n"), json!({ "normalized": true }))
        }
        other => {
            return Err(Error::InvalidArgument(format!(
                "unknown transform operation: {other}. Supported: dedupe_lines, sort_lines, filter_lines, head_lines, tail_lines, truncate_chars, add_line_numbers, count_lines, normalize_whitespace"
            )));
        }
    };

    let max_bytes = max_output_bytes();
    let truncated = output.len() > max_bytes;
    let final_content = if truncated {
        output.chars().take(max_bytes).collect::<String>()
    } else {
        output
    };

    Ok(json!({
        "operation": operation,
        "input_lines": input_lines.len(),
        "input_chars": content.len(),
        "output_lines": final_content.lines().count(),
        "output_chars": final_content.len(),
        "truncated": truncated,
        "max_output_bytes": max_bytes,
        "meta": meta,
        "content": final_content
    }))
}

pub fn supported_operations() -> Value {
    json!({
        "execution_model": "safe_builtin_ops",
        "operations": [
            { "name": "dedupe_lines", "description": "Remove duplicate lines preserving first occurrence" },
            { "name": "sort_lines", "params": { "reverse": false } },
            { "name": "filter_lines", "params": { "query": "required", "regex": false, "case_sensitive": true } },
            { "name": "head_lines", "params": { "n": 10 } },
            { "name": "tail_lines", "params": { "n": 10 } },
            { "name": "truncate_chars", "params": { "max": 4096 } },
            { "name": "add_line_numbers", "params": { "start": 1 } },
            { "name": "count_lines", "description": "Return line/char counts as content summary" },
            { "name": "normalize_whitespace", "description": "Trim trailing space and collapse blank runs" }
        ],
        "hint": "No arbitrary code execution. Use rlm_artifact_write to persist transform output."
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_and_filter_work() {
        let text = "a\nb\na\nc\n";
        let out = apply(text, "dedupe_lines", &json!({})).unwrap();
        assert_eq!(out["content"], "a\nb\nc");
        assert_eq!(out["meta"]["deduped"], 1);

        let filtered = apply(text, "filter_lines", &json!({ "query": "b" })).unwrap();
        assert_eq!(filtered["content"], "b");
    }

    #[test]
    fn truncates_oversized_output() {
        std::env::set_var("RLM_MAX_TRANSFORM_BYTES", "8");
        let out = apply("1234567890", "truncate_chars", &json!({ "max": 100 })).unwrap();
        assert!(out["truncated"].as_bool().unwrap());
        assert!(out["content"].as_str().unwrap().len() <= 8);
        std::env::remove_var("RLM_MAX_TRANSFORM_BYTES");
    }
}
