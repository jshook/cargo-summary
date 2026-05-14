// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! `cargo_summary` — assistant-facing wrapper around `cargo` that
//! filters output and emits a single concise line per invocation,
//! with optional passthrough / timeout / heartbeat for long runs.
//!
//! Without flags it captures cargo's full output, classifies the run
//! (build / test / clippy / check), and prints one summary line. With
//! flags it can additionally:
//!
//! - relay cargo's stdout/stderr verbatim (`--passthrough` or `-p`),
//! - kill the child after N seconds (`--timeout SECS`),
//! - emit periodic liveness lines on long-running invocations,
//!   triggered by either elapsed-time-since-last-heartbeat
//!   (`--heartbeat SECS`) or accumulated-lines-since-last-heartbeat
//!   (`--heartbeat-lines N`). Either or both can be set; whichever
//!   condition fires first emits a single status line and resets
//!   both counters.
//!
//! Output forms:
//!
//!   * `build` → `BUILD OK in 12.3s` or `BUILD FAILED: <first error line>`
//!   * `test`  → `TEST OK total=2074 passed=2063 failed=0 ignored=11 in 18.4s`
//!               or `TEST FAILED ... failures=[name1, name2]`
//!   * `clippy` → `CLIPPY OK warnings=N in 4.2s` / `CLIPPY FAILED ...`
//!
//! Test runner: when `cargo-nextest` is installed (the binary
//! `cargo-nextest` is on PATH), `test` mode invokes
//! `cargo nextest run --message-format libtest-json` and parses the
//! line-delimited JSON event stream — more robust than regexing
//! libtest's text output. Pass `--no-nextest` to force the legacy
//! `cargo test` path. The summary line shape is identical either way.
//!
//! ## Usage
//!
//!     cargo_summary [tool-flags] <cargo-subcommand> [cargo args...]
//!
//! Tool flags (must precede the cargo subcommand):
//!
//!     -p, --passthrough           Relay cargo output verbatim to terminal
//!     -v                          Alias for --passthrough
//!     --timeout SECS              Kill the child after SECS (no output → exit 124)
//!     --heartbeat SECS            Emit `[heartbeat ...]` every SECS to stderr
//!     --quiet-summary             Suppress the trailing summary line
//!     -h, --help                  Print this help and exit
//!
//! Examples:
//!
//!     cargo_summary test --workspace
//!     cargo_summary -p test -p veks-pipeline survey::orchestrator
//!     cargo_summary --timeout 600 --heartbeat 30 test --workspace
//!

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() {
    // When invoked as a cargo external subcommand (`cargo summary
    // ...`), cargo passes our own extension name as the first arg
    // (argv[1] == "summary"). Strip it so the parser sees the
    // intended cargo subcommand as the first non-flag token,
    // regardless of how we were invoked.
    let mut raw_args: Vec<String> = env::args().skip(1).collect();
    if raw_args.first().map(|s| s.as_str()) == Some("summary") {
        raw_args.remove(0);
    }
    let parsed = match parse_args(&raw_args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{}", msg);
            std::process::exit(2);
        }
    };
    if parsed.print_help {
        print_help();
        return;
    }
    let mode = parsed.cargo_args[0].clone();
    let started = Instant::now();

    // Decide the invocation:
    //   * `test` + nextest-on-path + not --no-nextest  → cargo nextest run …
    //   * everything else                              → cargo <mode> …
    let using_nextest =
        mode == "test" && !parsed.no_nextest && cargo_nextest_available();
    let (cmd_program, cmd_args, mut extra_env): (&str, Vec<String>, Vec<(&str, String)>) =
        if using_nextest {
            // Drop the leading "test" subcommand and pass the rest as
            // nextest args. Filter out flags that nextest doesn't share
            // with `cargo test` (none of these appear in our normal use,
            // but we keep the door open for future divergence).
            let mut args: Vec<String> = vec![
                "nextest".into(),
                "run".into(),
                "--message-format".into(),
                "libtest-json".into(),
                "--message-format-version".into(),
                "0.1".into(),
                "--no-fail-fast".into(),
            ];
            args.extend(parsed.cargo_args.iter().skip(1).cloned());
            (
                "cargo",
                args,
                vec![("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1".into())],
            )
        } else {
            ("cargo", parsed.cargo_args.clone(), Vec::new())
        };

    // Persistent diagnostic logs under target/cargo_summary/ so a
    // failed run can be re-inspected after the assistant has moved on.
    // Best-effort: failure to create the log files is non-fatal — we
    // just skip logging and emit the summary anyway.
    let log_dir = resolve_log_dir();
    let (mut stdout_log, mut stderr_log, stdout_log_path, stderr_log_path) =
        open_log_files(&log_dir, &mode);

    let mut cmd = Command::new(cmd_program);
    cmd.args(&cmd_args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in extra_env.drain(..) {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to spawn {}: {}", cmd_program, e);
            std::process::exit(2);
        }
    };

    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let tx_o = event_tx.clone();
    let stdout_handle = thread::spawn(move || {
        let r = BufReader::new(stdout);
        for line in r.lines().map_while(Result::ok) {
            if tx_o.send(Event::Stdout(line)).is_err() { break; }
        }
        let _ = tx_o.send(Event::StdoutEof);
    });
    let tx_e = event_tx.clone();
    let stderr_handle = thread::spawn(move || {
        let r = BufReader::new(stderr);
        for line in r.lines().map_while(Result::ok) {
            if tx_e.send(Event::Stderr(line)).is_err() { break; }
        }
        let _ = tx_e.send(Event::StderrEof);
    });
    drop(event_tx);

    let mut stdout_lines: Vec<String> = Vec::new();
    let mut stderr_lines: Vec<String> = Vec::new();
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let mut last_heartbeat = Instant::now();
    let mut lines_since_heartbeat: u64 = 0;
    let mut latest_status_line: Option<String> = None;
    let mut timed_out = false;

    // Pick the inner select interval. Want to wake often enough to
    // service the time-based heartbeat promptly and to re-check
    // overall timeout, but not busy-loop.
    let select_interval = Duration::from_millis(
        parsed
            .heartbeat_secs
            .map(|s| (s * 1000).min(1_000).max(100))
            .unwrap_or(1_000),
    );

    loop {
        if stdout_eof && stderr_eof { break; }
        if let Some(deadline) = parsed.timeout_secs {
            if started.elapsed().as_secs_f64() >= deadline as f64 {
                timed_out = true;
                let _ = child.kill();
                break;
            }
        }
        match event_rx.recv_timeout(select_interval) {
            Ok(Event::Stdout(line)) => {
                if parsed.passthrough {
                    let _ = writeln!(std::io::stdout().lock(), "{}", line);
                }
                if let Some(f) = stdout_log.as_mut() {
                    let _ = writeln!(f, "{}", line);
                }
                if !line.trim().is_empty() {
                    latest_status_line = Some(line.clone());
                }
                stdout_lines.push(line);
                lines_since_heartbeat += 1;
            }
            Ok(Event::Stderr(line)) => {
                if parsed.passthrough {
                    let _ = writeln!(std::io::stderr().lock(), "{}", line);
                }
                if let Some(f) = stderr_log.as_mut() {
                    let _ = writeln!(f, "{}", line);
                }
                if !line.trim().is_empty() {
                    latest_status_line = Some(line.clone());
                }
                stderr_lines.push(line);
                lines_since_heartbeat += 1;
            }
            Ok(Event::StdoutEof) => stdout_eof = true,
            Ok(Event::StderrEof) => stderr_eof = true,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // After every event AND every recv-timeout, check whether a
        // heartbeat is due. Either trigger resets both counters.
        let line_trigger = parsed
            .heartbeat_lines
            .map(|n| lines_since_heartbeat >= n)
            .unwrap_or(false);
        let time_trigger = parsed
            .heartbeat_secs
            .map(|s| last_heartbeat.elapsed() >= Duration::from_secs(s))
            .unwrap_or(false);
        if line_trigger || time_trigger {
            emit_heartbeat(
                started,
                stdout_lines.len() as u64,
                stderr_lines.len() as u64,
                lines_since_heartbeat,
                latest_status_line.as_deref(),
            );
            last_heartbeat = Instant::now();
            lines_since_heartbeat = 0;
        }
    }

    // Drain remaining events that beat the channel close, then await.
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();
    let status = wait_with_timeout(&mut child, parsed.timeout_secs, started);
    let elapsed = started.elapsed().as_secs_f64();

    let summary = if timed_out {
        format!(
            "TIMEOUT after {:.1}s (limit {}s) — child killed",
            elapsed,
            parsed.timeout_secs.unwrap_or(0)
        )
    } else {
        match mode.as_str() {
            "test" => {
                if using_nextest {
                    summarize_test_nextest(&stdout_lines, &stderr_lines, status.success, elapsed)
                } else {
                    summarize_test(&stdout_lines, &stderr_lines, status.success, elapsed)
                }
            }
            "build" | "check" => summarize_build(&stderr_lines, status.success, elapsed, &mode),
            "clippy" => summarize_clippy(&stderr_lines, status.success, elapsed),
            _ => summarize_build(&stderr_lines, status.success, elapsed, &mode),
        }
    };

    if !parsed.quiet_summary {
        // Flush logs before emitting summary so post-hoc readers see
        // the complete capture regardless of buffering.
        if let Some(f) = stdout_log.as_mut() { let _ = f.flush(); }
        if let Some(f) = stderr_log.as_mut() { let _ = f.flush(); }
        let logs = match (stdout_log_path.as_ref(), stderr_log_path.as_ref()) {
            (Some(o), Some(e)) => format!(" logs={} {}", o.display(), e.display()),
            _ => String::new(),
        };
        println!("{}{}", summary, logs);
    }

    if timed_out {
        std::process::exit(124);
    }
    if !status.success {
        std::process::exit(status.code.unwrap_or(1));
    }
}

// ---------------------------------------------------------------------------
// CLI parsing
// ---------------------------------------------------------------------------

struct ParsedArgs {
    passthrough: bool,
    timeout_secs: Option<u64>,
    /// Time-based heartbeat: fire after N seconds without firing.
    heartbeat_secs: Option<u64>,
    /// Line-based heartbeat: fire after N cumulative lines on
    /// stdout+stderr without firing.
    heartbeat_lines: Option<u64>,
    /// When true, the `test` mode invokes plain `cargo test` and
    /// regex-parses libtest stdout instead of using
    /// `cargo nextest run --message-format libtest-json`.
    no_nextest: bool,
    quiet_summary: bool,
    print_help: bool,
    cargo_args: Vec<String>,
}

fn parse_args(raw: &[String]) -> Result<ParsedArgs, String> {
    let mut p = ParsedArgs {
        passthrough: false,
        timeout_secs: None,
        heartbeat_secs: None,
        heartbeat_lines: None,
        no_nextest: false,
        quiet_summary: false,
        print_help: false,
        cargo_args: Vec::new(),
    };
    let mut i = 0;
    while i < raw.len() {
        let arg = raw[i].as_str();
        match arg {
            "-h" | "--help" => { p.print_help = true; return Ok(p); }
            "-p" | "-v" | "--passthrough" => p.passthrough = true,
            "--quiet-summary" => p.quiet_summary = true,
            "--no-nextest" => p.no_nextest = true,
            "--timeout" => {
                i += 1;
                let v = raw
                    .get(i)
                    .ok_or_else(|| "--timeout requires a value in seconds".to_string())?;
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--timeout value '{}' is not an integer", v))?;
                p.timeout_secs = Some(n);
            }
            "--heartbeat" | "--heartbeat-secs" => {
                i += 1;
                let v = raw
                    .get(i)
                    .ok_or_else(|| "--heartbeat requires a value in seconds".to_string())?;
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--heartbeat value '{}' is not an integer", v))?;
                p.heartbeat_secs = Some(n);
            }
            "--heartbeat-lines" => {
                i += 1;
                let v = raw
                    .get(i)
                    .ok_or_else(|| "--heartbeat-lines requires a line count".to_string())?;
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--heartbeat-lines value '{}' is not an integer", v))?;
                p.heartbeat_lines = Some(n);
            }
            _ => {
                // First non-flag token is the cargo subcommand; the
                // rest are its args.
                p.cargo_args = raw[i..].to_vec();
                break;
            }
        }
        i += 1;
    }
    if p.print_help { return Ok(p); }
    if p.cargo_args.is_empty() {
        return Err("usage: cargo_summary [tool-flags] <cargo-subcommand> [cargo args...]".into());
    }
    Ok(p)
}

fn print_help() {
    let help = r#"cargo_summary — concise cargo wrapper

Tool flags (must precede the cargo subcommand):
    -p, --passthrough           Relay cargo output verbatim
    -v                          Alias for --passthrough
    --timeout SECS              Kill the child after SECS (exit 124)
    --heartbeat SECS            Emit a liveness line after SECS of silence
    --heartbeat-secs SECS       Alias for --heartbeat
    --heartbeat-lines N         Emit a liveness line after every N output lines
                                (fires on whichever heartbeat condition trips first)
    --no-nextest                Force `cargo test` for `test` mode even when
                                cargo-nextest is installed
    --quiet-summary             Suppress the trailing summary line
    -h, --help                  Print this help

Examples:
    cargo_summary test --workspace
    cargo_summary -p test -p veks-pipeline survey::orchestrator
    cargo_summary --timeout 600 --heartbeat 30 test --workspace
"#;
    eprint!("{}", help);
}

// ---------------------------------------------------------------------------
// Child waiting
// ---------------------------------------------------------------------------

struct ExitInfo { success: bool, code: Option<i32> }

fn wait_with_timeout(child: &mut Child, timeout_secs: Option<u64>, started: Instant) -> ExitInfo {
    if timeout_secs.is_none() {
        let s = child.wait().expect("wait");
        return ExitInfo { success: s.success(), code: s.code() };
    }
    let limit = Duration::from_secs(timeout_secs.unwrap());
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return ExitInfo { success: s.success(), code: s.code() },
            Ok(None) => {
                if started.elapsed() >= limit {
                    let _ = child.kill();
                    let s = child.wait().expect("wait after kill");
                    return ExitInfo { success: false, code: s.code() };
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return ExitInfo { success: false, code: None },
        }
    }
}

// ---------------------------------------------------------------------------
// Heartbeat rendering
// ---------------------------------------------------------------------------

fn emit_heartbeat(
    started: Instant,
    total_stdout: u64,
    total_stderr: u64,
    lines_since_last: u64,
    latest_line: Option<&str>,
) {
    let elapsed = started.elapsed().as_secs_f64();
    let snippet = latest_line
        .map(|s| {
            let trimmed: String = s.chars().take(120).collect();
            if s.chars().count() > 120 {
                format!("{}…", trimmed)
            } else {
                trimmed
            }
        })
        .unwrap_or_default();
    let trailer = if snippet.is_empty() {
        String::new()
    } else {
        format!(" latest=\"{}\"", snippet)
    };
    eprintln!(
        "[heartbeat {:.0}s stdout={} stderr={} +{} since-last{}]",
        elapsed, total_stdout, total_stderr, lines_since_last, trailer
    );
}

// ---------------------------------------------------------------------------
// Diagnostic log files
// ---------------------------------------------------------------------------

/// Resolve `<workspace-root>/target/cargo_summary` by asking cargo
/// where the workspace root is, falling back to `./target/cargo_summary`
/// when the resolution fails.
fn resolve_log_dir() -> PathBuf {
    // Cheapest reliable signal: walk up from CWD looking for a
    // Cargo.toml that declares a [workspace]. Failing that, just use
    // CWD/target.
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut probe = cwd.clone();
    let workspace_root = loop {
        let candidate = probe.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                if text.contains("[workspace]") {
                    break Some(probe.clone());
                }
            }
        }
        if !probe.pop() { break None; }
    };
    let root = workspace_root.unwrap_or(cwd);
    root.join("target").join("cargo_summary")
}

fn open_log_files(
    log_dir: &PathBuf,
    mode: &str,
) -> (Option<File>, Option<File>, Option<PathBuf>, Option<PathBuf>) {
    if let Err(_) = std::fs::create_dir_all(log_dir) {
        return (None, None, None, None);
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    // Filename: `<mode>-<ts>.stdout.log`. Stable, sortable.
    let stdout_path = log_dir.join(format!("{}-{}.stdout.log", mode, ts));
    let stderr_path = log_dir.join(format!("{}-{}.stderr.log", mode, ts));
    let stdout_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&stdout_path)
        .ok();
    let stderr_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&stderr_path)
        .ok();
    (
        stdout_file,
        stderr_file,
        Some(stdout_path),
        Some(stderr_path),
    )
}

// ---------------------------------------------------------------------------
// Event from reader threads
// ---------------------------------------------------------------------------

enum Event {
    Stdout(String),
    Stderr(String),
    StdoutEof,
    StderrEof,
}

// ---------------------------------------------------------------------------
// nextest detection
// ---------------------------------------------------------------------------

/// True iff `cargo nextest` is callable. Used to decide whether to
/// route `test` mode through nextest's JSON output. Cached on first
/// call via a `OnceLock`.
fn cargo_nextest_available() -> bool {
    use std::sync::OnceLock;
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        Command::new("cargo")
            .arg("nextest")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// nextest libtest-json summary
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum NextestEvent {
    Suite(SuiteEvent),
    Test(TestEvent),
    #[serde(other)]
    Other,
}

#[derive(serde::Deserialize)]
struct SuiteEvent {
    event: String,
    #[serde(default)]
    passed: u64,
    #[serde(default)]
    failed: u64,
    #[serde(default)]
    ignored: u64,
}

#[derive(serde::Deserialize)]
struct TestEvent {
    event: String,
    #[serde(default)]
    name: String,
}

fn summarize_test_nextest(
    stdout: &[String],
    stderr: &[String],
    ok: bool,
    secs: f64,
) -> String {
    // Aggregate counts from one or more `suite` events; capture
    // failure names from per-test `event=failed` lines.
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut ignored = 0u64;
    let mut suite_seen = false;
    let mut failure_names: Vec<String> = Vec::new();
    let mut compile_errors: Vec<String> = Vec::new();

    for line in stdout {
        if !line.starts_with('{') { continue; }
        let parsed: Result<NextestEvent, _> = serde_json::from_str(line);
        match parsed {
            Ok(NextestEvent::Suite(s)) if s.event == "ok" || s.event == "failed" => {
                passed += s.passed;
                failed += s.failed;
                ignored += s.ignored;
                suite_seen = true;
            }
            Ok(NextestEvent::Test(t)) if t.event == "failed" => {
                if failure_names.len() < 8 {
                    // Nextest names look like
                    // `crate::module::test_name`; strip the
                    // `binary$` prefix that libtest-json sometimes
                    // includes.
                    let name = t.name.split('$').last().unwrap_or(&t.name).to_string();
                    failure_names.push(name);
                }
            }
            _ => {}
        }
    }

    // Build / compile errors flow to stderr (cargo's, not nextest's).
    for line in stderr {
        if line.starts_with("error[") || line.starts_with("error: ") {
            compile_errors.push(line.clone());
            if compile_errors.len() > 6 { break; }
        }
    }
    if !compile_errors.is_empty() && !suite_seen {
        return format!(
            "TEST BUILD-FAILED in {:.1}s [{}]",
            secs,
            compile_errors.join(" | ")
        );
    }

    let total = passed + failed + ignored;
    if ok && failed == 0 {
        format!(
            "TEST OK total={} passed={} failed={} ignored={} in {:.1}s [nextest]",
            total, passed, failed, ignored, secs
        )
    } else {
        let fail_list = if failure_names.is_empty() {
            String::new()
        } else {
            format!(" failures=[{}]", failure_names.join(", "))
        };
        format!(
            "TEST FAILED total={} passed={} failed={} ignored={}{} in {:.1}s [nextest]",
            total, passed, failed, ignored, fail_list, secs
        )
    }
}

// ---------------------------------------------------------------------------
// test summary
// ---------------------------------------------------------------------------

fn summarize_test(stdout: &[String], stderr: &[String], ok: bool, secs: f64) -> String {
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut ignored = 0u64;
    let mut failure_names: Vec<String> = Vec::new();
    let mut compile_errors: Vec<String> = Vec::new();

    for line in stdout {
        if let Some(rest) = line.strip_prefix("test result: ") {
            for token in rest.split(';') {
                let token = token.trim();
                if let Some(rest) = token.strip_suffix(" passed") {
                    if let Some(num) = rest.split_whitespace().last() {
                        passed += num.parse::<u64>().unwrap_or(0);
                    }
                } else if let Some(rest) = token.strip_suffix(" failed") {
                    if let Some(num) = rest.split_whitespace().last() {
                        failed += num.parse::<u64>().unwrap_or(0);
                    }
                } else if let Some(rest) = token.strip_suffix(" ignored") {
                    if let Some(num) = rest.split_whitespace().last() {
                        ignored += num.parse::<u64>().unwrap_or(0);
                    }
                }
            }
        }
        if let Some(rest) = line.strip_prefix("test ") {
            if rest.contains(" ... FAILED") {
                let name = rest.split(" ... FAILED").next().unwrap_or("").to_string();
                if failure_names.len() < 8 {
                    failure_names.push(name);
                }
            }
        }
    }

    for line in stderr {
        if line.starts_with("error[") || line.starts_with("error: ") {
            compile_errors.push(line.clone());
            if compile_errors.len() > 6 { break; }
        }
    }

    if !compile_errors.is_empty() {
        return format!(
            "TEST BUILD-FAILED in {:.1}s [{}]",
            secs,
            compile_errors.join(" | ")
        );
    }

    let total = passed + failed + ignored;
    if ok && failed == 0 {
        format!(
            "TEST OK total={} passed={} failed={} ignored={} in {:.1}s",
            total, passed, failed, ignored, secs
        )
    } else {
        let fail_list = if failure_names.is_empty() {
            String::new()
        } else {
            format!(" failures=[{}]", failure_names.join(", "))
        };
        format!(
            "TEST FAILED total={} passed={} failed={} ignored={}{} in {:.1}s",
            total, passed, failed, ignored, fail_list, secs
        )
    }
}

// ---------------------------------------------------------------------------
// build / check summary
// ---------------------------------------------------------------------------

fn summarize_build(stderr: &[String], ok: bool, secs: f64, mode: &str) -> String {
    let mode_upper = mode.to_ascii_uppercase();
    if ok {
        let warning_count = stderr.iter().filter(|l| l.starts_with("warning:")).count();
        let warns = if warning_count > 0 {
            format!(" warnings={}", warning_count)
        } else {
            String::new()
        };
        return format!("{} OK{} in {:.1}s", mode_upper, warns, secs);
    }
    let errors: Vec<&String> = stderr
        .iter()
        .filter(|l| l.starts_with("error[") || l.starts_with("error: "))
        .take(6)
        .collect();
    if errors.is_empty() {
        return format!("{} FAILED in {:.1}s [no error lines captured]", mode_upper, secs);
    }
    let joined = errors
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    format!("{} FAILED in {:.1}s [{}]", mode_upper, secs, joined)
}

// ---------------------------------------------------------------------------
// clippy summary
// ---------------------------------------------------------------------------

fn summarize_clippy(stderr: &[String], ok: bool, secs: f64) -> String {
    let warnings: Vec<&String> = stderr
        .iter()
        .filter(|l| l.starts_with("warning:"))
        .take(6)
        .collect();
    let warn_count = stderr.iter().filter(|l| l.starts_with("warning:")).count();
    if ok {
        return format!("CLIPPY OK warnings={} in {:.1}s", warn_count, secs);
    }
    let errors: Vec<&String> = stderr
        .iter()
        .filter(|l| l.starts_with("error[") || l.starts_with("error: "))
        .take(6)
        .collect();
    let first = if !errors.is_empty() {
        errors.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" | ")
    } else if !warnings.is_empty() {
        warnings.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" | ")
    } else {
        "no diagnostic lines captured".into()
    };
    format!("CLIPPY FAILED warnings={} in {:.1}s [{}]", warn_count, secs, first)
}
