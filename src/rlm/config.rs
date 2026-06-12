use std::env;

const DEFAULT_MAX_FILE_BYTES: u64 = 512 * 1024;
const DEFAULT_MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_CHUNKS: usize = 10_000;
const DEFAULT_MAX_SESSIONS: usize = 50;
const DEFAULT_SESSION_TTL_SECS: u64 = 3600;
const DEFAULT_CHUNK_LINES: usize = 200;

#[derive(Debug, Clone, Copy)]
pub struct RlmConfig {
    pub max_file_bytes: u64,
    pub max_total_bytes: usize,
    pub max_chunks: usize,
    pub max_sessions: usize,
    pub session_ttl_secs: u64,
    pub chunk_lines: usize,
}

impl Default for RlmConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl RlmConfig {
    pub fn from_env() -> Self {
        Self {
            max_file_bytes: parse_env_u64("RLM_MAX_FILE_BYTES", DEFAULT_MAX_FILE_BYTES),
            max_total_bytes: parse_env_usize("RLM_MAX_TOTAL_BYTES", DEFAULT_MAX_TOTAL_BYTES),
            max_chunks: parse_env_usize("RLM_MAX_CHUNKS", DEFAULT_MAX_CHUNKS),
            max_sessions: parse_env_usize("RLM_MAX_SESSIONS", DEFAULT_MAX_SESSIONS),
            session_ttl_secs: parse_env_u64("RLM_SESSION_TTL_SECS", DEFAULT_SESSION_TTL_SECS),
            chunk_lines: parse_env_usize("RLM_CHUNK_LINES", DEFAULT_CHUNK_LINES),
        }
    }
}

fn parse_env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = RlmConfig {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_chunks: DEFAULT_MAX_CHUNKS,
            max_sessions: DEFAULT_MAX_SESSIONS,
            session_ttl_secs: DEFAULT_SESSION_TTL_SECS,
            chunk_lines: DEFAULT_CHUNK_LINES,
        };
        assert!(cfg.max_file_bytes > 0);
        assert!(cfg.max_total_bytes > cfg.max_file_bytes as usize);
    }
}
