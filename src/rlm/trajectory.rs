use crate::error::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryEvent {
    pub seq: u64,
    pub ts_unix: u64,
    pub event_type: String,
    pub session_id: String,
    pub task_id: Option<String>,
    pub detail: Value,
    pub bytes_in: usize,
    pub bytes_out: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryRun {
    pub session_id: String,
    pub events: Vec<TrajectoryEvent>,
    pub started_at_unix: u64,
}

pub struct TrajectoryStore {
    runs: HashMap<String, TrajectoryRun>,
}

impl TrajectoryStore {
    pub fn new() -> Self {
        let mut runs = HashMap::new();
        if let Ok(loaded) = load_all_runs() {
            for run in loaded {
                runs.insert(run.session_id.clone(), run);
            }
        }
        Self { runs }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        session_id: &str,
        event_type: &str,
        task_id: Option<&str>,
        detail: Value,
        bytes_in: usize,
        bytes_out: usize,
        started: Instant,
    ) {
        let run = self
            .runs
            .entry(session_id.to_string())
            .or_insert_with(|| TrajectoryRun {
                session_id: session_id.to_string(),
                events: Vec::new(),
                started_at_unix: unix_now(),
            });
        let seq = run.events.len() as u64 + 1;
        run.events.push(TrajectoryEvent {
            seq,
            ts_unix: unix_now(),
            event_type: event_type.into(),
            session_id: session_id.into(),
            task_id: task_id.map(|s| s.to_string()),
            detail,
            bytes_in,
            bytes_out,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
        let _ = append_jsonl_event(session_id, run.events.last().unwrap());
    }

    pub fn run(&self, session_id: &str) -> Option<TrajectoryRun> {
        self.runs.get(session_id).cloned()
    }

    pub fn get(
        &self,
        session_id: &str,
        format: &str,
        redact: bool,
        redact_patterns: &[String],
    ) -> Result<Value> {
        let run = self.runs.get(session_id).ok_or_else(|| {
            crate::error::Error::InvalidArgument(format!("no trajectory for session: {session_id}"))
        })?;

        let events: Vec<Value> = run
            .events
            .iter()
            .map(|e| {
                if redact {
                    redact_event(e, redact_patterns)
                } else {
                    serde_json::to_value(e).unwrap_or(json!({}))
                }
            })
            .collect();

        let summary = summarize_run(run);

        match format {
            "jsonl" => {
                let lines: Vec<String> = events.iter().map(|e| e.to_string()).collect();
                Ok(json!({
                    "session_id": session_id,
                    "format": "jsonl",
                    "summary": summary,
                    "jsonl": lines.join("\n"),
                    "event_count": events.len(),
                }))
            }
            "replay" => Ok(json!({
                "session_id": session_id,
                "format": "replay",
                "summary": summary,
                "replay_steps": build_replay_steps(&events),
                "events": events,
            })),
            _ => Ok(json!({
                "session_id": session_id,
                "format": "json",
                "summary": summary,
                "events": events,
            })),
        }
    }
}

impl Default for TrajectoryStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn summarize_run(run: &TrajectoryRun) -> Value {
    let mut by_type: HashMap<String, u64> = HashMap::new();
    let mut total_bytes_in = 0usize;
    let mut total_bytes_out = 0usize;
    let mut chunks_read = 0u64;
    let mut sub_calls = 0u64;
    let mut max_depth = 0u64;
    let mut errors = 0u64;
    let mut wall_ms = 0u64;

    for e in &run.events {
        *by_type.entry(e.event_type.clone()).or_default() += 1;
        total_bytes_in += e.bytes_in;
        total_bytes_out += e.bytes_out;
        wall_ms += e.elapsed_ms;
        if e.event_type == "chunk" || e.event_type == "map" {
            chunks_read += e
                .detail
                .get("chunks_returned")
                .or_else(|| e.detail.get("chunk_count"))
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
        }
        if e.event_type == "sub_call" {
            sub_calls += 1;
            max_depth = max_depth.max(e.detail.get("depth").and_then(|v| v.as_u64()).unwrap_or(0));
        }
        if e.event_type == "error" || e.event_type == "budget" {
            errors += 1;
        }
    }

    let elapsed_secs = run
        .events
        .last()
        .map(|e| e.ts_unix.saturating_sub(run.started_at_unix))
        .unwrap_or(0);

    json!({
        "event_count": run.events.len(),
        "by_type": by_type,
        "total_bytes_in": total_bytes_in,
        "total_bytes_out": total_bytes_out,
        "chunks_read": chunks_read,
        "sub_call_count": sub_calls,
        "max_recursion_depth": max_depth,
        "error_count": errors,
        "wall_elapsed_ms": wall_ms,
        "wall_elapsed_secs": elapsed_secs,
        "tail_cost_note": "High variance possible on recursive runs — inspect sub_call and budget events"
    })
}

fn build_replay_steps(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .map(|e| {
            let event_type = e.get("event_type").and_then(|v| v.as_str()).unwrap_or("?");
            let hint = match event_type {
                "scan" | "load" => "Reload context if session expired",
                "peek" | "filter" => "Re-run filter with same query/path/glob",
                "chunk" | "map" => "Re-read same chunk_ids at same offset/limit",
                "sub_call" => "Re-create task with same prompt and chunk_ids",
                "reduce" => "Re-merge worker outputs or task_reduce",
                "budget" | "error" => "Inspect failure and narrow scope before retry",
                "final_answer" => "Terminal step — compare with current answer",
                _ => "Review event detail",
            };
            json!({
                "seq": e.get("seq"),
                "event_type": event_type,
                "replay_hint": hint,
                "detail_keys": e.get("detail").and_then(|d| d.as_object()).map(|o| {
                    o.keys().cloned().collect::<Vec<_>>()
                }),
            })
        })
        .collect()
}

fn redact_event(event: &TrajectoryEvent, patterns: &[String]) -> Value {
    let mut value = serde_json::to_value(event).unwrap_or(json!({}));
    redact_value(&mut value, patterns);
    value
}

fn redact_value(value: &mut Value, patterns: &[String]) {
    match value {
        Value::String(s) => {
            *s = redact_string(s, patterns);
        }
        Value::Array(arr) => {
            for item in arr {
                redact_value(item, patterns);
            }
        }
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if key.contains("content")
                    || key.contains("prompt")
                    || key.contains("preview")
                    || key.contains("output")
                {
                    if let Value::String(s) = val {
                        *val = Value::String(redact_string(s, patterns));
                    }
                } else {
                    redact_value(val, patterns);
                }
            }
        }
        _ => {}
    }
}

fn redact_string(s: &str, patterns: &[String]) -> String {
    let mut out = s.to_string();
    if out.len() > 200 {
        out = format!("{}...[redacted {} bytes]", &out[..120], s.len());
    }
    out = super::safety::redact_secrets(&out, patterns);
    out
}

fn trajectories_dir() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-trajectories")
}

fn jsonl_path(session_id: &str) -> PathBuf {
    trajectories_dir().join(format!("{session_id}.jsonl"))
}

fn append_jsonl_event(session_id: &str, event: &TrajectoryEvent) -> Result<()> {
    let dir = trajectories_dir();
    std::fs::create_dir_all(&dir)?;
    let path = jsonl_path(session_id);
    let line = serde_json::to_string(event)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    file.sync_all()?;
    Ok(())
}

pub fn load_all_runs() -> Result<Vec<TrajectoryRun>> {
    let dir = trajectories_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let content = std::fs::read_to_string(&path)?;
        let mut events = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<TrajectoryEvent>(line) {
                events.push(event);
            }
        }
        if !events.is_empty() {
            let started_at_unix = events.first().map(|e| e.ts_unix).unwrap_or_else(unix_now);
            runs.push(TrajectoryRun {
                session_id,
                events,
                started_at_unix,
            });
        }
    }
    Ok(runs)
}

pub fn detail_size(v: &Value) -> usize {
    serde_json::to_string(v).map(|s| s.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;
    use tempfile::TempDir;

    #[test]
    fn records_and_summarizes_events() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("RLM_CACHE_DIR", cache.path());

        let mut store = TrajectoryStore::new();
        let started = Instant::now();
        store.record(
            "sess-1",
            "scan",
            None,
            json!({"chunk_count": 3}),
            100,
            50,
            started,
        );
        store.record(
            "sess-1",
            "peek",
            None,
            json!({"match_count": 2}),
            10,
            20,
            started,
        );

        let out = store.get("sess-1", "json", true, &[]).unwrap();
        assert_eq!(out["summary"]["event_count"].as_u64().unwrap(), 2);
        assert_eq!(out["summary"]["by_type"]["scan"].as_u64().unwrap(), 1);

        std::env::remove_var("RLM_CACHE_DIR");
    }

    #[test]
    fn redacts_default_secret_markers() {
        let event = TrajectoryEvent {
            seq: 1,
            ts_unix: 0,
            event_type: "task".into(),
            session_id: "s".into(),
            task_id: None,
            detail: json!({"output": "Authorization: Bearer sk-testtoken password=secret"}),
            bytes_in: 0,
            bytes_out: 0,
            elapsed_ms: 0,
        };
        let redacted = redact_event(&event, &[]);
        let output = redacted["detail"]["output"].as_str().unwrap();
        assert!(!output.contains("Bearer "));
        assert!(!output.contains("sk-"));
        assert!(!output.contains("password="));
    }

    #[test]
    fn redacts_long_content() {
        let event = TrajectoryEvent {
            seq: 1,
            ts_unix: 0,
            event_type: "peek".into(),
            session_id: "s".into(),
            task_id: None,
            detail: json!({"preview": "x".repeat(500)}),
            bytes_in: 0,
            bytes_out: 0,
            elapsed_ms: 0,
        };
        let redacted = redact_event(&event, &[]);
        let preview = redacted["detail"]["preview"].as_str().unwrap();
        assert!(preview.contains("redacted"));
    }
}
