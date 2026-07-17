//! Request-scoped cancellation for blocking tool workers.
//!
//! MCP router installs a [`CancellationToken`] on the `spawn_blocking` thread so
//! command providers and REPL sandboxes can poll and kill child processes.

use crate::error::{Error, Result};
use std::cell::RefCell;
use tokio_util::sync::CancellationToken;

thread_local! {
    static CURRENT: RefCell<Option<CancellationToken>> = const { RefCell::new(None) };
}

/// Installs `token` for the current thread until dropped.
pub struct CancelGuard;

impl CancelGuard {
    pub fn install(token: CancellationToken) -> Self {
        CURRENT.with(|slot| {
            *slot.borrow_mut() = Some(token);
        });
        Self
    }
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

/// Returns true when the current blocking request was cancelled.
pub fn is_cancelled() -> bool {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(CancellationToken::is_cancelled)
            .unwrap_or(false)
    })
}

/// Fail fast if the current request was cancelled.
pub fn check() -> Result<()> {
    if is_cancelled() {
        Err(Error::Cancelled("request cancelled".into()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn check_ok_without_token() {
        assert!(check().is_ok());
        assert!(!is_cancelled());
    }

    #[test]
    fn check_fails_when_cancelled() {
        let token = CancellationToken::new();
        let _guard = CancelGuard::install(token.clone());
        assert!(check().is_ok());
        token.cancel();
        assert!(is_cancelled());
        let err = check().unwrap_err();
        assert!(matches!(err, Error::Cancelled(_)));
    }

    #[test]
    fn guard_clears_on_drop() {
        let token = CancellationToken::new();
        token.cancel();
        {
            let _guard = CancelGuard::install(token);
            assert!(is_cancelled());
        }
        assert!(!is_cancelled());
        // avoid unused import warning on some toolchains
        let _ = Duration::from_millis(1);
        let _ = thread::available_parallelism();
    }
}
