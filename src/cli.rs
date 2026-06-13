use crate::error::{Error, Result};
use crate::rlm::PeekOptions;
use crate::rlm::RlmEngine;
use serde_json::{json, Value};
use std::io::{self, Read};

pub fn run_cli(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::InvalidArgument(
            "usage: rlm-mcp <command> [options]\n\
             commands: install, scan, peek, chunk, env-info, slice, transform, repl-info, repl-exec, artifact-write, artifact-read, \
             map-plan, map-claim, map-complete, reduce-schema, reduce-merge, \
             session-list, session-delete, session-cleanup, session-export, session-import, \
             task-create, task-list, task-result, task-reduce, \
             trajectory-get, trajectory-final, budget-configure, budget-status, task-cancel, \
             benchmark, tools-reference, workflow"
                .into(),
        ));
    }

    let command = args[0].as_str();
    if matches!(command, "--version" | "-V" | "version") {
        println!("rlm-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let flags = parse_flags(&args[1..]);
    let json_mode = flags.get_bool("json") || flags.get_bool("quiet");
    let engine = RlmEngine::new();

    let result = match command {
        "install" => {
            let binary = std::env::current_exe()?;
            let configured = crate::install::configure_agents(&binary)?;
            json!({
                "installed": true,
                "binary": binary,
                "configured": configured,
                "restart_required": true
            })
        }
        "scan" => {
            let path = flags.get_str("path");
            let content = flags
                .get_str("content")
                .map(|s| s.to_string())
                .or_else(|| read_stdin_content(&flags).ok())
                .map(|s| s.to_string());
            engine.scan(
                path,
                content.as_deref(),
                flags.get_str("virtual-path"),
                flags.get_str("variable"),
            )?
        }
        "peek" => {
            let session_id = flags.require_str("session-id")?;
            engine.peek(
                session_id,
                PeekOptions {
                    query: flags.get_str("query"),
                    path_filter: flags.get_str("path"),
                    glob: flags.get_str("glob"),
                    regex: flags.get_bool("regex"),
                    bm25: flags.get_bool("bm25"),
                    case_sensitive: !flags.get_bool("ignore-case"),
                    line_start: flags.get_usize("line-start"),
                    line_end: flags.get_usize("line-end"),
                    context_radius: flags.get_usize("context").unwrap_or(2),
                    limit: flags.get_usize("limit").unwrap_or(20),
                    include_content: flags.get_bool("full"),
                },
            )?
        }
        "chunk" => engine.chunk(
            flags.require_str("session-id")?,
            flags.get_str("file-pattern"),
            flags.get_str_list("chunk-id").as_deref(),
            flags.get_usize("offset").unwrap_or(0),
            flags.get_usize("limit").unwrap_or(5),
            flags.get_bool("metadata"),
        )?,
        "env-info" => engine.env_info(flags.require_str("session-id")?)?,
        "slice" => engine.slice(
            flags.require_str("session-id")?,
            flags.require_str("chunk-id")?,
            flags.get_usize("start").unwrap_or(1),
            flags.get_usize("end").unwrap_or(1),
        )?,
        "transform" => {
            let params = flags
                .get_str("params")
                .map(serde_json::from_str::<Value>)
                .transpose()?
                .unwrap_or_else(|| json!({}));
            engine.transform(
                flags.require_str("session-id")?,
                flags.require_str("op")?,
                &params,
                flags.get_str("chunk-id"),
                flags.get_str("artifact"),
                flags.get_str("content"),
            )?
        }
        "repl-info" => engine.repl_info(),
        "repl-exec" => engine.repl_execute(
            flags.require_str("session-id")?,
            flags.require_str("code")?,
            flags.get_str("language"),
            flags.get_str("backend"),
        )?,
        "artifact-write" => engine.artifact_write(
            flags.require_str("session-id")?,
            flags.require_str("name")?,
            flags.get_str("content"),
            flags.get_str("chunk-id"),
        )?,
        "artifact-read" => engine.artifact_read(
            flags.require_str("session-id")?,
            flags.require_str("name")?,
            flags.get_usize("start"),
            flags.get_usize("end"),
        )?,
        "map-plan" => engine.map_plan(
            flags.require_str("session-id")?,
            flags.get_str_list("chunk-id").as_deref(),
            flags.get_str("file-pattern"),
            flags.get_usize("batch-size").unwrap_or(3),
        )?,
        "map-claim" => engine.map_claim(
            flags.require_str("plan-id")?,
            flags.require_str("worker-id")?,
            flags.get_str("batch-id"),
        )?,
        "map-complete" => {
            let output = flags
                .get_str("output")
                .map(serde_json::from_str::<Value>)
                .transpose()?
                .ok_or_else(|| Error::InvalidArgument("provide --output JSON".into()))?;
            engine.map_complete(
                flags.require_str("plan-id")?,
                flags.require_str("worker-id")?,
                flags.require_str("batch-id")?,
                output,
            )?
        }
        "reduce-schema" => engine.reduce_schema(),
        "reduce-merge" => {
            let workers = flags
                .get_str("workers")
                .map(serde_json::from_str::<Vec<Value>>)
                .transpose()?
                .unwrap_or_default();
            engine.reduce_merge(&workers)?
        }
        "session-list" => engine.session_list(),
        "session-delete" => engine.session_delete(flags.require_str("session-id")?)?,
        "session-cleanup" => engine.session_cleanup()?,
        "session-export" => engine.session_export(flags.require_str("session-id")?)?,
        "session-import" => {
            let json_str = flags
                .get_str("session-json")
                .map(|s| s.to_string())
                .or_else(|| read_stdin_content(&flags).ok())
                .ok_or_else(|| {
                    Error::InvalidArgument("provide --session-json or --stdin".into())
                })?;
            let parsed: serde_json::Value = serde_json::from_str(&json_str)?;
            let session: crate::rlm::ScanSession = if let Some(inner) = parsed.get("session") {
                serde_json::from_value(inner.clone())?
            } else {
                serde_json::from_value(parsed)?
            };
            engine.session_import(session, flags.get_bool("preserve-id"))?
        }
        "task-create" => engine.task_create(
            flags.require_str("session-id")?,
            flags.require_str("prompt")?,
            &flags.get_str_list("chunk-id").unwrap_or_default(),
            flags.get_str("parent-task-id"),
            flags.get_str("provider").unwrap_or("mock"),
            None,
            None,
            !flags.get_bool("no-execute"),
        )?,
        "task-list" => engine.task_list(flags.get_str("session-id"), flags.get_str("root-id")),
        "task-result" => engine.task_result(flags.require_str("task-id")?)?,
        "task-reduce" => engine.task_reduce(flags.require_str("root-id")?)?,
        "trajectory-get" => engine.trajectory_get(
            flags.require_str("session-id")?,
            flags.get_str("format").unwrap_or("json"),
            !flags.get_bool("no-redact"),
            &flags.get_str_list("redact-pattern").unwrap_or_default(),
        )?,
        "budget-configure" => {
            use crate::rlm::{BudgetMode, SessionBudget, TaskBudget};
            let session_id = flags.require_str("session-id")?;
            let mode = if flags.get_bool("soft-warning") {
                BudgetMode::SoftWarning
            } else {
                BudgetMode::FailFast
            };
            engine.budget_configure(SessionBudget {
                session_id: session_id.to_string(),
                mode,
                max_chunks_read: flags.get_usize("max-chunks").unwrap_or(500) as u64,
                max_sub_calls: flags.get_usize("max-sub-calls").unwrap_or(64) as u64,
                max_total_tokens_est: flags.get_usize("max-tokens").unwrap_or(500_000) as u64,
                max_wall_secs: flags.get_usize("max-wall-secs").unwrap_or(600) as u64,
                task_budget: TaskBudget::default(),
            })?
        }
        "budget-status" => engine.budget_status(flags.require_str("session-id")?),
        "task-cancel" => engine.task_cancel(
            flags.require_str("root-id")?,
            flags.get_str("reason").unwrap_or("cancelled by agent"),
        )?,
        "trajectory-final" => engine.trajectory_record_final(
            flags.require_str("session-id")?,
            flags.require_str("answer")?,
            flags.get_usize("evidence-count").unwrap_or(0),
        ),
        "benchmark" => {
            let sub = flags
                .get_str("suite")
                .or_else(|| {
                    args.get(1)
                        .filter(|s| !s.starts_with("--"))
                        .map(|s| s.as_str())
                })
                .unwrap_or("list");
            match sub {
                "list" => crate::benchmark::list_suites(),
                "run" => crate::benchmark::run_suite(
                    &engine,
                    flags.get_str("name").unwrap_or("sniah"),
                    flags.get_str("size"),
                )?,
                other => crate::benchmark::run_suite(&engine, other, flags.get_str("size"))?,
            }
        }
        "tools-reference" => crate::mcp::schema_docs::tools_reference(),
        "workflow" => engine.workflow(flags.get_str("phase").unwrap_or("overview")),
        _ => {
            return Err(Error::InvalidArgument(format!(
                "unknown command: {command}"
            )))
        }
    };

    if json_mode {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}

fn read_stdin_content(flags: &Flags) -> Result<String> {
    if flags.get_bool("stdin") {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        Err(Error::InvalidArgument("no content".into()))
    }
}

struct Flags {
    values: std::collections::HashMap<String, Vec<String>>,
}

impl Flags {
    fn get_str(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|v| v.last())
            .map(|s| s.as_str())
    }

    fn get_str_list(&self, key: &str) -> Option<Vec<String>> {
        self.values.get(key).cloned()
    }

    fn get_bool(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    fn get_usize(&self, key: &str) -> Option<usize> {
        self.get_str(key).and_then(|v| v.parse().ok())
    }

    fn require_str(&self, key: &str) -> Result<&str> {
        self.get_str(key)
            .ok_or_else(|| Error::InvalidArgument(format!("missing --{key}")))
    }
}

fn parse_flags(args: &[String]) -> Flags {
    let mut values: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(key) = arg.strip_prefix("--") {
            let (name, inline_val) = match key.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (key.to_string(), None),
            };
            let val = if let Some(v) = inline_val {
                v
            } else if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                i += 1;
                args[i].clone()
            } else {
                "true".into()
            };
            values.entry(name).or_default().push(val);
        }
        i += 1;
    }
    Flags { values }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags() {
        let args = vec![
            "--session-id".into(),
            "abc".into(),
            "--limit".into(),
            "5".into(),
            "--json".into(),
        ];
        let flags = parse_flags(&args);
        assert_eq!(flags.get_str("session-id"), Some("abc"));
        assert_eq!(flags.get_usize("limit"), Some(5));
        assert!(flags.get_bool("json"));
    }
}
