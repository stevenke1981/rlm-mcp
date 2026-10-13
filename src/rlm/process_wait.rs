//! Concurrent child I/O with a deadline and request cancellation.

use crate::error::{Error, Result};
use crate::rlm::cancel;
use std::process::{Command, Output};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const DEFAULT_PROVIDER_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub enum OutputLimit {
    Fail(usize),
    Truncate(usize),
}

pub struct CapturedOutput {
    pub output: Output,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Run a command, draining both output pipes while sending optional stdin.
/// The wall limit covers stdin, process exit, and output collection. Errors,
/// cancellation, and timeouts kill and reap the immediate child and close its
/// pipes. Descendants are not managed as a process group.
pub fn run_command(
    command: Command,
    input: Option<&[u8]>,
    max_wall: Option<Duration>,
    label: &str,
) -> Result<Output> {
    run_command_limited(
        command,
        input,
        max_wall,
        label,
        OutputLimit::Fail(provider_max_output()?),
    )
    .map(|captured| captured.output)
}

pub fn run_command_limited(
    command: Command,
    input: Option<&[u8]>,
    max_wall: Option<Duration>,
    label: &str,
    limit: OutputLimit,
) -> Result<CapturedOutput> {
    cancel::check()?;
    let token = cancel::current_token();
    let run = move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(run_command_async(
            command, input, max_wall, label, token, limit,
        ))
    };
    // Synchronous providers may also be called from a current-thread Tokio
    // runtime. A scoped thread avoids nested block_on / block_in_place panics.
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| {
            scope
                .spawn(run)
                .join()
                .map_err(|_| Error::Other(format!("{label} I/O worker panicked")))?
        })
    } else {
        run()
    }
}

async fn read_pipe(
    pipe: Option<impl AsyncRead + Unpin>,
    policy: OutputLimit,
    stream: &str,
) -> Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    let limit = match policy {
        OutputLimit::Fail(limit) | OutputLimit::Truncate(limit) => limit,
    };
    let mut truncated = false;
    if let Some(mut pipe) = pipe {
        let mut buffer = [0u8; 8192];
        loop {
            let count = pipe.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            let available = limit.saturating_sub(bytes.len());
            if count > available {
                if matches!(policy, OutputLimit::Fail(_)) {
                    return Err(Error::BudgetExceeded(format!(
                        "{stream} exceeded capture limit of {limit} bytes"
                    )));
                }
                truncated = true;
            }
            bytes.extend_from_slice(&buffer[..count.min(available)]);
        }
    }
    Ok((bytes, truncated))
}

async fn run_command_async(
    command: Command,
    input: Option<&[u8]>,
    max_wall: Option<Duration>,
    label: &str,
    token: Option<CancellationToken>,
    limit: OutputLimit,
) -> Result<CapturedOutput> {
    let mut child = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| Error::Other(format!("failed to spawn {label}: {error}")))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let outcome = {
        let io = async {
            let write = async {
                if let (Some(mut stdin), Some(input)) = (stdin, input) {
                    // Commands that produce a result without consuming all
                    // input may close stdin early; their exit status is decisive.
                    if let Err(error) = stdin.write_all(input).await {
                        if error.kind() != std::io::ErrorKind::BrokenPipe {
                            return Err(Error::Io(error));
                        }
                    }
                }
                Ok(())
            };
            let (_, (stdout, stdout_truncated), (stderr, stderr_truncated), status) = tokio::try_join!(
                write,
                read_pipe(stdout, limit, "stdout"),
                read_pipe(stderr, limit, "stderr"),
                async { child.wait().await.map_err(Error::Io) }
            )?;
            Ok(CapturedOutput {
                output: Output {
                    status,
                    stdout,
                    stderr,
                },
                stdout_truncated,
                stderr_truncated,
            })
        };
        let cancelled = async {
            match token {
                Some(token) => token.cancelled().await,
                None => std::future::pending().await,
            }
        };
        let deadline = async {
            match max_wall {
                Some(limit) => tokio::time::sleep(limit).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            _ = cancelled => Err(Error::Cancelled(format!("{label} cancelled"))),
            _ = deadline => Err(Error::Other(format!(
                "{label} exceeded wall limit of {}s", max_wall.unwrap().as_secs_f64()
            ))),
            result = io => result,
        }
    };
    if outcome.is_err() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    outcome
}

/// Positive byte cap per command output pipe and HTTP response body.
pub fn provider_max_output() -> Result<usize> {
    parse_output_limit(
        std::env::var("RLM_PROVIDER_MAX_OUTPUT_BYTES")
            .ok()
            .as_deref(),
    )
}

fn parse_output_limit(raw: Option<&str>) -> Result<usize> {
    match raw {
        None => Ok(DEFAULT_PROVIDER_MAX_OUTPUT_BYTES),
        Some(raw) => raw
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                Error::InvalidArgument(
                    "RLM_PROVIDER_MAX_OUTPUT_BYTES must be a positive integer".into(),
                )
            }),
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
    use std::io::{Read, Write};
    use std::path::Path;
    use std::process::Stdio;
    use std::thread;
    use std::time::Instant;

    #[test]
    #[ignore = "internal child fixture, launched by process I/O regression tests"]
    fn output_fixture() {
        match std::env::var("RLM_TEST_IO_FIXTURE").as_deref() {
            Ok("output") | Ok("duplex") => {
                std::io::stdout()
                    .write_all(&vec![b'o'; 512 * 1024])
                    .unwrap();
                std::io::stderr()
                    .write_all(&vec![b'e'; 512 * 1024])
                    .unwrap();
                if std::env::var("RLM_TEST_IO_FIXTURE").unwrap() == "duplex" {
                    let mut input = Vec::new();
                    std::io::stdin().read_to_end(&mut input).unwrap();
                    assert_eq!(input, vec![b'i'; 512 * 1024]);
                }
            }
            Ok("oversized_stdout") | Ok("oversized_stderr") => {
                let ready = std::env::var_os("RLM_TEST_IO_READY").unwrap();
                std::fs::write(&ready, "ready").unwrap();
                let bytes = vec![b'x'; 2 * 1024 * 1024];
                if std::env::var("RLM_TEST_IO_FIXTURE").unwrap() == "oversized_stdout" {
                    std::io::stdout().write_all(&bytes).unwrap();
                } else {
                    std::io::stderr().write_all(&bytes).unwrap();
                }
                thread::sleep(Duration::from_secs(1));
                std::fs::write(Path::new(&ready).with_extension("survived"), "survived").unwrap();
            }
            Ok("blocked") => {
                let ready = std::env::var_os("RLM_TEST_IO_READY").unwrap();
                std::fs::write(&ready, "ready").unwrap();
                thread::sleep(Duration::from_secs(1));
                std::fs::write(Path::new(&ready).with_extension("survived"), "survived").unwrap();
            }
            _ => {}
        }
    }

    fn fixture(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "rlm::process_wait::tests::output_fixture",
                "--nocapture",
            ])
            .env("RLM_TEST_IO_FIXTURE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn assert_output(output: Output) {
        assert!(output.status.success());
        assert!(output
            .stdout
            .windows(512 * 1024)
            .any(|bytes| bytes.iter().all(|b| *b == b'o')));
        assert_eq!(output.stderr, vec![b'e'; 512 * 1024]);
    }

    #[test]
    fn output_limit_requires_a_positive_integer() {
        assert_eq!(parse_output_limit(None).unwrap(), 1024 * 1024);
        assert_eq!(parse_output_limit(Some("4096")).unwrap(), 4096);
        for value in ["0", "-1", "", "invalid"] {
            assert!(parse_output_limit(Some(value)).is_err());
        }
    }

    #[test]
    fn truncation_drains_both_pipes_without_growing_capture_buffers() {
        let captured = run_command_limited(
            fixture("output"),
            None,
            Some(Duration::from_secs(3)),
            "truncate",
            OutputLimit::Truncate(4096),
        )
        .unwrap();
        assert!(captured.output.status.success());
        assert_eq!(captured.output.stdout.len(), 4096);
        assert_eq!(captured.output.stderr.len(), 4096);
        assert!(captured.stdout_truncated && captured.stderr_truncated);
    }

    #[test]
    fn drains_large_stdout_and_stderr_while_child_runs() {
        assert_output(
            run_command(
                fixture("output"),
                None,
                Some(Duration::from_secs(3)),
                "large-output",
            )
            .unwrap(),
        );
    }

    #[test]
    fn sends_stdin_while_draining_both_output_pipes() {
        assert_output(
            run_command(
                fixture("duplex"),
                Some(&vec![b'i'; 512 * 1024]),
                Some(Duration::from_secs(3)),
                "duplex",
            )
            .unwrap(),
        );
    }

    #[test]
    fn cancellation_bounds_blocked_stdin_and_reaps_child() {
        let cache = tempfile::TempDir::new().unwrap();
        let ready = cache.path().join("ready");
        let token = CancellationToken::new();
        let _guard = CancelGuard::install(token.clone());
        let notify_path = ready.clone();
        let notify = thread::spawn(move || {
            let started = Instant::now();
            while !notify_path.exists() && started.elapsed() < Duration::from_secs(3) {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(notify_path.exists(), "child must start before cancellation");
            token.cancel();
        });
        let mut command = fixture("blocked");
        command.env("RLM_TEST_IO_READY", &ready);
        let started = Instant::now();
        let error = run_command(
            command,
            Some(&vec![b'i'; 512 * 1024]),
            Some(Duration::from_secs(5)),
            "blocked-stdin",
        )
        .unwrap_err();
        notify.join().unwrap();
        assert!(matches!(error, Error::Cancelled(_)), "{error}");
        assert!(started.elapsed() < Duration::from_secs(3));
        thread::sleep(Duration::from_millis(1100));
        assert!(
            !ready.with_extension("survived").exists(),
            "cancelled child must not survive"
        );
    }

    #[test]
    fn kills_on_cancel_before_spawn() {
        let token = CancellationToken::new();
        let _guard = CancelGuard::install(token.clone());
        token.cancel();
        let error = run_command(
            Command::new("nonexistent-rlm-test-program"),
            None,
            None,
            "cancelled",
        )
        .unwrap_err();
        assert!(matches!(error, Error::Cancelled(_)), "{error}");
    }

    #[test]
    fn timeout_bounds_blocked_stdin_and_reaps_child() {
        let cache = tempfile::TempDir::new().unwrap();
        let ready = cache.path().join("ready");
        let mut command = fixture("blocked");
        command.env("RLM_TEST_IO_READY", &ready);
        let started = Instant::now();
        let error = run_command(
            command,
            Some(&vec![b'i'; 512 * 1024]),
            Some(Duration::from_millis(300)),
            "slow-command",
        )
        .unwrap_err();
        assert!(error.to_string().contains("wall limit"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(ready.exists(), "child must start before deadline");
        thread::sleep(Duration::from_millis(1100));
        assert!(
            !ready.with_extension("survived").exists(),
            "timed out child must not survive"
        );
    }

    #[test]
    fn rejects_oversized_stdout_and_reaps_child() {
        assert_oversized_output_rejected("oversized_stdout");
    }

    #[test]
    fn rejects_oversized_stderr_and_reaps_child() {
        assert_oversized_output_rejected("oversized_stderr");
    }

    fn assert_oversized_output_rejected(mode: &str) {
        let cache = tempfile::TempDir::new().unwrap();
        let ready = cache.path().join("ready");
        let mut command = fixture(mode);
        command.env("RLM_TEST_IO_READY", &ready);
        let outcome = run_command(command, None, Some(Duration::from_secs(3)), "oversized");
        assert!(
            matches!(outcome, Err(Error::BudgetExceeded(_))),
            "oversized output must be rejected"
        );
        assert!(ready.exists(), "child must have produced the output");
        thread::sleep(Duration::from_millis(1100));
        assert!(
            !ready.with_extension("survived").exists(),
            "oversized child must not survive"
        );
    }

    #[test]
    fn collects_output_on_success() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd.exe");
            command.args(["/C", "echo", "hello-wait"]);
            command
        } else {
            let mut command = Command::new("echo");
            command.arg("hello-wait");
            command
        };
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = run_command(command, None, Some(Duration::from_secs(5)), "echo").unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("hello-wait"));
    }

    #[tokio::test]
    async fn works_inside_current_thread_runtime() {
        assert_output(
            run_command(
                fixture("output"),
                None,
                Some(Duration::from_secs(3)),
                "nested-runtime",
            )
            .unwrap(),
        );
    }
}
