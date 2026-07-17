//! Session load/filter/REPL/artifact ops on [`super::RlmEngine`].

use super::RlmEngine;
use crate::error::{Error, Result};
use crate::rlm::artifacts;
use crate::rlm::env;
use crate::rlm::filter::{self, PeekOptions};
use crate::rlm::glob_match;
use crate::rlm::repl;
use crate::rlm::safety;
use crate::rlm::session::{ScanSession, SessionStore};
use crate::rlm::trajectory;
use crate::rlm::transform;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Instant;

impl RlmEngine {
    pub fn scan(
        &self,
        path: Option<&str>,
        content: Option<&str>,
        virtual_path: Option<&str>,
        variable_name: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let mut store = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let session = match (path, content) {
            (Some(p), None) | (Some(p), Some(_)) => store.create_from_path(p)?,
            (None, Some(text)) => {
                let vp = virtual_path.unwrap_or("inline.txt");
                let mut vars = HashMap::new();
                if let Some(name) = variable_name {
                    vars.insert(name.to_string(), text.to_string());
                }
                store.create_from_text(text, vp, vars)?
            }
            (None, None) => {
                return Err(Error::InvalidArgument("provide path or content".into()));
            }
        };

        let out = json!({
            "session_id": session.id,
            "root_path": session.root_path,
            "source_kind": session.source_kind,
            "file_count": session.files_scanned,
            "chunk_count": session.chunks.len(),
            "total_bytes": session.total_bytes,
            "files_scanned": session.files_scanned,
            "files_skipped": session.files_skipped,
            "skip_reasons": session.skip_reasons,
            "variables": session.variables.keys().collect::<Vec<_>>(),
            "created_at_unix": session.created_at_unix,
            "expires_at_unix": session.expires_at_unix,
            "hint": "Use rlm_env_info to inspect, rlm_peek to filter, rlm_chunk to read"
        });
        self.record(
            &session.id,
            "scan",
            None,
            json!({
                "source_kind": session.source_kind,
                "chunk_count": session.chunks.len(),
                "total_bytes": session.total_bytes,
                "files_scanned": session.files_scanned,
            }),
            path.map(|p| p.len()).unwrap_or(0) + content.map(|c| c.len()).unwrap_or(0),
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn env_info(&self, session_id: &str) -> Result<Value> {
        let started = Instant::now();
        let session = self.session_snapshot(session_id)?;
        let mut out = env::env_info(&session);
        if let Some(obj) = out.as_object_mut() {
            obj.insert("repl".into(), repl::list_backends());
        }
        self.record(
            session_id,
            "load",
            None,
            json!({
                "chunk_count": out["chunk_count"],
                "file_count": out["file_count"],
                "context_length_bytes": out["context_length_bytes"],
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn slice(
        &self,
        session_id: &str,
        chunk_id: &str,
        start_line: usize,
        end_line: usize,
    ) -> Result<Value> {
        let started = Instant::now();
        let session = self.session_snapshot(session_id)?;
        let chunk = session
            .chunks
            .iter()
            .find(|c| c.id == chunk_id)
            .ok_or_else(|| Error::InvalidArgument(format!("chunk not found: {chunk_id}")))?
            .clone();
        let body = SessionStore::resolve_chunk_content(session_id, &chunk)?;
        let out = env::slice_chunk(&chunk, &body, start_line, end_line);
        self.record(
            session_id,
            "slice",
            None,
            json!({
                "chunk_id": chunk_id,
                "start_line": start_line,
                "end_line": end_line,
                "line_count": out["line_count"],
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    fn resolve_text_input(
        &self,
        session_id: &str,
        chunk_id: Option<&str>,
        artifact_name: Option<&str>,
        content: Option<&str>,
    ) -> Result<String> {
        if let Some(text) = content {
            return Ok(text.to_string());
        }
        if let Some(name) = artifact_name {
            let read = artifacts::read_artifact(session_id, name, None, None)?;
            return read
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| Error::Other("artifact read missing content".into()));
        }
        if let Some(id) = chunk_id {
            let session = self.session_snapshot(session_id)?;
            let chunk = session
                .chunks
                .iter()
                .find(|c| c.id == id)
                .ok_or_else(|| Error::InvalidArgument(format!("chunk not found: {id}")))?;
            return SessionStore::resolve_chunk_content(session_id, chunk);
        }
        Err(Error::InvalidArgument(
            "provide content, artifact_name, or chunk_id".into(),
        ))
    }

    pub fn transform(
        &self,
        session_id: &str,
        operation: &str,
        params: &Value,
        chunk_id: Option<&str>,
        artifact_name: Option<&str>,
        content: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let input = self.resolve_text_input(session_id, chunk_id, artifact_name, content)?;
        let input_len = input.len();
        let out =
            repl::ReplBackend::execute_transform(&repl::safe_backend(), &input, operation, params)?;
        self.record(
            session_id,
            "transform",
            None,
            json!({
                "backend": "safe_builtin",
                "operation": operation,
                "input_chars": input_len,
                "output_chars": out.get("output_chars"),
                "truncated": out.get("truncated"),
            }),
            input_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn transform_operations(&self) -> Value {
        transform::supported_operations()
    }

    pub fn repl_info(&self) -> Value {
        repl::list_backends()
    }

    pub fn repl_execute(
        &self,
        session_id: &str,
        code: &str,
        language: Option<&str>,
        backend: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let backend_id = backend
            .and_then(repl::ReplBackendId::parse)
            .unwrap_or(repl::ReplBackendId::Command);

        if backend_id == repl::ReplBackendId::SafeBuiltin {
            return Err(Error::InvalidArgument(
                "repl_execute requires an executable backend (command or python)".into(),
            ));
        }

        let exec_backend: Box<dyn repl::ReplBackend> = match backend_id {
            repl::ReplBackendId::SafeBuiltin => Box::new(repl::SafeBuiltinBackend),
            repl::ReplBackendId::Command => Box::new(repl::CommandSandboxBackend::new(
                repl::SandboxLimits::from_env(),
            )),
            repl::ReplBackendId::Python => {
                return Err(Error::InvalidArgument(
                    "python REPL backend is not implemented; use backend=command".into(),
                ));
            }
        };

        let lang = language.unwrap_or("text");
        let code_len = code.len();
        let out = exec_backend.execute_code(session_id, code, lang)?;
        let wall_ms = started.elapsed().as_millis() as u64;

        self.record(
            session_id,
            "repl_exec",
            None,
            json!({
                "backend": exec_backend.name(),
                "language": lang,
                "input_bytes": code_len,
                "output_bytes": out.get("output_chars"),
                "wall_ms": wall_ms,
                "truncated": out.get("truncated"),
                "audit": out.get("audit"),
            }),
            code_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn artifact_write(
        &self,
        session_id: &str,
        name: &str,
        content: Option<&str>,
        source_chunk_id: Option<&str>,
    ) -> Result<Value> {
        let started = Instant::now();
        let body = if let Some(text) = content {
            text.to_string()
        } else if let Some(chunk_id) = source_chunk_id {
            let session = self.session_snapshot(session_id)?;
            let chunk = session
                .chunks
                .iter()
                .find(|c| c.id == chunk_id)
                .ok_or_else(|| Error::InvalidArgument(format!("chunk not found: {chunk_id}")))?;
            SessionStore::resolve_chunk_content(session_id, chunk)?
        } else {
            return Err(Error::InvalidArgument(
                "provide content or source_chunk_id".into(),
            ));
        };
        let byte_len = body.len();
        let out = artifacts::write_artifact(session_id, name, &body)?;
        self.record(
            session_id,
            "artifact_write",
            None,
            json!({
                "name": out.get("name"),
                "bytes": byte_len,
            }),
            byte_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn artifact_read(
        &self,
        session_id: &str,
        name: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<Value> {
        let started = Instant::now();
        let out = artifacts::read_artifact(session_id, name, start_line, end_line)?;
        let bytes = out.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        self.record(
            session_id,
            "artifact_read",
            None,
            json!({
                "name": name,
                "bytes": bytes,
                "start_line": start_line,
                "end_line": end_line,
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn chunk(
        &self,
        session_id: &str,
        file_pattern: Option<&str>,
        chunk_ids: Option<&[String]>,
        offset: usize,
        limit: usize,
        _include_metadata: bool,
    ) -> Result<Value> {
        let started = Instant::now();
        let budget_eval = self.ensure_session_budget(session_id, limit as u64, 0, 0)?;
        // Snapshot releases the store lock before body I/O — concurrent chunk readers.
        let session = self.session_snapshot(session_id)?;
        let filtered: Vec<_> = session
            .chunks
            .iter()
            .filter(|c| {
                if let Some(ids) = chunk_ids {
                    if !ids.contains(&c.id) {
                        return false;
                    }
                }
                file_pattern.is_none_or(|pat| {
                    c.path.contains(pat) || c.path.ends_with(pat) || glob_match(pat, &c.path)
                })
            })
            .collect();

        let page: Vec<_> = filtered.iter().skip(offset).take(limit).collect();
        let max_chunk_bytes = safety::max_chunk_output_bytes();
        let mut any_truncated = false;
        let chunks: Vec<Value> = page
            .iter()
            .map(|c| {
                let body = SessionStore::resolve_chunk_content(session_id, c).unwrap_or_default();
                let (content, truncated) = safety::truncate_chunk_content(&body);
                if truncated {
                    any_truncated = true;
                }
                json!({
                    "id": c.id,
                    "path": c.path,
                    "offset": c.offset,
                    "line_count": c.line_count,
                    "content": content,
                    "content_lazy": c.content_file.is_some() && c.content.is_empty(),
                    "truncated": truncated,
                    "max_chunk_bytes": max_chunk_bytes
                })
            })
            .collect();

        let mut out = json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "total": filtered.len(),
            "chunk_ids": page.iter().map(|c| &c.id).collect::<Vec<_>>(),
            "chunks": chunks,
            "max_chunk_bytes": max_chunk_bytes,
            "any_truncated": any_truncated
        });
        if !budget_eval.warnings.is_empty() {
            out["budget_warnings"] = json!(budget_eval.warnings);
        }
        self.record(
            session_id,
            "chunk",
            None,
            json!({
                "offset": offset,
                "limit": limit,
                "chunks_returned": page.len(),
                "chunk_ids": page.iter().map(|c| &c.id).collect::<Vec<_>>(),
            }),
            0,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn peek(&self, session_id: &str, opts: PeekOptions<'_>) -> Result<Value> {
        let started = Instant::now();
        let query_len = opts.query.map(|q| q.len()).unwrap_or(0);
        let session = self.session_snapshot(session_id)?;
        let out = filter::peek_session(&session, opts);
        self.record(
            session_id,
            "peek",
            None,
            json!({
                "query": out.get("query"),
                "returned": out.get("returned"),
                "total_match_lines": out.get("total_match_lines"),
                "truncated": out.get("truncated"),
            }),
            query_len,
            trajectory::detail_size(&out),
            started,
        );
        Ok(out)
    }

    pub fn session_list(&self) -> Value {
        let store = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        json!({ "sessions": store.list() })
    }

    pub fn session_delete(&self, session_id: &str) -> Result<Value> {
        self.sessions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .delete(session_id)?;
        Ok(json!({ "session_id": session_id, "deleted": true }))
    }

    pub fn session_cleanup(&self) -> Result<Value> {
        let report = self
            .sessions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .cleanup_expired()?;
        Ok(json!({
            "removed_count": report.removed_count,
            "removed_ids": report.removed_ids,
        }))
    }

    pub fn session_export(&self, session_id: &str) -> Result<Value> {
        let session = self
            .sessions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .export(session_id)?;
        Ok(json!({
            "session_id": session.id,
            "revision": session.revision,
            "session": serde_json::to_value(session)?,
        }))
    }

    pub fn session_import(&self, session: ScanSession, preserve_id: bool) -> Result<Value> {
        let imported = self
            .sessions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .import_session(session, preserve_id)?;
        Ok(json!({
            "session_id": imported.id,
            "revision": imported.revision,
            "chunk_count": imported.chunks.len(),
            "total_bytes": imported.total_bytes,
        }))
    }
}
