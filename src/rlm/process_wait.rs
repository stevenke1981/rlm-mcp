//! Poll a child process with wall timeout and request cancellation support.

use crate::error::{Error, Result};
use crate::rlm::cancel;
use std::io::Read;
use std::process::{Child, Output};
use std::thread;
use std::time::{Duration, Instant};

/// Wait for `child` until exit, wall timeout, or request cancellation.
///
/// On timeout or cancel the child is killed. Stdout/stderr are collected when
/// the process exits normally.
pub fn wait_child(child: &mut Child, max_wall: Option<Duration>, label: &str) -> Result<Output> {
    let started = Instant::now();
    loop {
        if let Err(e) = cancel::check() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(match e {
                Error::Cancelled(_) => Error::Cancelled(format!("{label} cancelled")),
                other => other,
            });
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    out.read_to_end(&mut stdout).map_err(Error::Io)?;
                }
                if let Some(mut err) = child.stderr.take() {
                    err.read_to_end(&mut stderr).map_err(Error::Io)?;
                }
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if let Some(limit) = max_wall {
                    if started.elapsed() > limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(Error::Other(format!(
                            "{label} exceeded wall limit of {}s",
                            limit.as_secs()
                        )));
                    }
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
}

/// Parse `RLM_PROVIDER_MAX_WALL_SECS` (default 300). `0` means no limit.
pub fn provider_max_wall() -> Option<Duration> {
    match std::env::var("RLM_PROVIDER_MAX_WALL_SECS") {
        Ok(v) => {
            let secs: u64 = v.parse().unwrap_or(300);
            if secs == 0 {
                None
            } else {
                Some(Duration::from_secs(secs))
            }
        }
        Err(_) => Some(Duration::from_secs(300)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rlm::cancel::CancelGuard;
    use std::process::{Command, Stdio};
    use tokio_util::sync::CancellationToken;

    #[test]
    fn kills_on_cancel() {
        let mut child = if cfg!(windows) {
            Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn ping")
        } else {
            Command::new("sleep")
                .arg("30")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn sleep")
        };

        let token = CancellationToken::new();
        let guard = CancelGuard::install(token.clone());
        token.cancel();
        let err = wait_child(&mut child, Some(Duration::from_secs(60)), "test").unwrap_err();
        drop(guard);
        assert!(
            matches!(err, Error::Cancelled(_)) || err.to_string().contains("cancelled"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn kills_on_timeout() {
        let mut child = if cfg!(windows) {
            Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn ping")
        } else {
            Command::new("sleep")
                .arg("30")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn sleep")
        };

        let started = Instant::now();
        let err =
            wait_child(&mut child, Some(Duration::from_millis(200)), "slow-command").unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            err.to_string().contains("wall limit") || err.to_string().contains("exceeded"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn collects_output_on_success() {
        let mut child = if cfg!(windows) {
            Command::new("cmd.exe")
                .args(["/C", "echo", "hello-wait"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn echo")
        } else {
            Command::new("echo")
                .arg("hello-wait")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn echo")
        };
        let out = wait_child(&mut child, Some(Duration::from_secs(5)), "echo").unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello-wait"));
    }
}
