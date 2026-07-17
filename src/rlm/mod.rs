mod artifacts;
mod bm25;
mod bm25_index;
mod budget;
pub(crate) mod cancel;
mod chunk_store;
mod config;
mod engine;
mod env;
mod filter;
mod map;
mod map_ledger;
mod persistence;
mod process_wait;
mod provider;
mod reduce;
mod repl;
mod safety;
mod session;
mod task;
mod trajectory;
mod transform;
mod workflow;

pub use cancel::CancelGuard;
pub use engine::RlmEngine;

pub use budget::{BudgetMode, SessionBudget};
pub use config::RlmConfig;
pub use filter::PeekOptions;
pub use provider::{DryRunProvider, MockProvider, ProviderResult};
pub use session::*;
pub use task::{RlmTask, TaskBudget, TaskStatus};
pub use workflow::*;

pub(crate) fn glob_match(pattern: &str, path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    simple_glob(pattern, file_name) || simple_glob(pattern, path)
}

fn simple_glob(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text.contains(pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut start = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text.starts_with(part) {
                return false;
            }
            start = part.len();
        } else if i == parts.len() - 1 {
            if !text[start..].ends_with(part) {
                return false;
            }
        } else if let Some(pos) = text[start..].find(part) {
            start += pos + part.len();
        } else {
            return false;
        }
    }
    true
}
