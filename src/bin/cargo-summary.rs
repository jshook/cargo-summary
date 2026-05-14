// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Binary entry point for `cargo-summary`. The parser/type/output-
//! documentation primitives live in the library half; this file
//! handles argument parsing, process spawning, the I/O capture
//! pipeline, heartbeat emission, log files, and exit-code routing.
//!
//! ## Operating modes
//!
//! - **Wrapped** -- for `build` / `check` / `test` / `clippy`:
//!   captures cargo's stdout/stderr, parses it, and emits one
//!   summary line.
//! - **Raw passthrough** -- default for any other cargo subcommand:
//!   spawns cargo with inherited stdio and propagates the exit code.
//! - **Wrapped-without-summary** -- opt in with `--wrap-unknown`:
//!   applies heartbeat / timeout / log capture without parsing.

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cargo_summary::{
    OUTPUT_DOC_SCHEMA, SubcommandKind, Summary, TestRunner, cargo_nextest_available, cargo_version,
    nextest_version, output_doc_json, output_doc_text, relativize_to_cwd, summarize_build,
    summarize_clippy, summarize_test_legacy, summarize_test_nextest,
};

const WRAPPER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let mut raw_args: Vec<String> = env::args().skip(1).collect();
    // When invoked as `cargo summary ...`, cargo passes our extension
    // name as argv[1]. Strip it so the parser sees the cargo
    // subcommand as the first non-flag token.
    if raw_args.first().map(std::string::String::as_str) == Some("summary") {
        raw_args.remove(0);
    }

    let parsed = match parse_args(&raw_args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };

    if parsed.print_help {
        print_help();
        return ExitCode::SUCCESS;
    }
    if parsed.print_output_doc {
        print!("{}", output_doc_text(WRAPPER_VERSION));
        return ExitCode::SUCCESS;
    }
    if parsed.print_output_doc_json {
        println!("{}", output_doc_json(WRAPPER_VERSION));
        return ExitCode::SUCCESS;
    }
    if parsed.print_output_doc_schema {
        println!("{OUTPUT_DOC_SCHEMA}");
        return ExitCode::SUCCESS;
    }
    if parsed.print_version_info {
        print!("{}", version_info_text());
        return ExitCode::SUCCESS;
    }

    let subcommand = parsed.cargo_args[0].clone();
    let kind = SubcommandKind::for_name(&subcommand);

    match (kind, parsed.wrap_unknown) {
        (Some(k), _) => run_wrapped(&parsed, Some(k)),
        (None, true) => run_wrapped(&parsed, None),
        (None, false) => {
            let mut offenders: Vec<&str> = Vec::new();
            if parsed.timeout_secs.is_some() {
                offenders.push("--timeout");
            }
            if parsed.heartbeat_secs.is_some() {
                offenders.push("--heartbeat");
            }
            if parsed.heartbeat_lines.is_some() {
                offenders.push("--heartbeat-lines");
            }
            if parsed.passthrough {
                offenders.push("--passthrough");
            }
            if parsed.quiet_summary {
                offenders.push("--quiet-summary");
            }
            if !offenders.is_empty() {
                eprintln!(
                    "cargo-summary: subcommand `{}` is not in the summarizable set ({}).\n  \
                     The flags {} require the wrapper machinery.\n  \
                     Pass --wrap-unknown to apply heartbeat/timeout/log capture without\n  \
                     a summary, or remove those flags to forward `cargo {}` verbatim.",
                    subcommand,
                    SubcommandKind::ALL_NAMES.join(", "),
                    offenders.join(", "),
                    subcommand,
                );
                return ExitCode::from(2);
            }
            run_raw(&parsed.cargo_args)
        }
    }
}

// ============================================================================
// CLI parsing
// ============================================================================

// Flags collected from argv. Several bool fields are unavoidable for a
// CLI flags struct; clippy::struct_excessive_bools fires at the count,
// not the shape.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
struct ParsedArgs {
    passthrough: bool,
    timeout_secs: Option<u64>,
    heartbeat_secs: Option<u64>,
    heartbeat_lines: Option<u64>,
    no_nextest: bool,
    quiet_summary: bool,
    wrap_unknown: bool,
    absolute_log_paths: bool,
    print_help: bool,
    print_output_doc: bool,
    print_output_doc_json: bool,
    print_output_doc_schema: bool,
    print_version_info: bool,
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
        wrap_unknown: false,
        absolute_log_paths: false,
        print_help: false,
        print_output_doc: false,
        print_output_doc_json: false,
        print_output_doc_schema: false,
        print_version_info: false,
        cargo_args: Vec::new(),
    };
    let mut i = 0;
    while i < raw.len() {
        let arg = raw[i].as_str();
        match arg {
            "-h" | "--help" => {
                p.print_help = true;
                return Ok(p);
            }
            "--describe-output" => {
                p.print_output_doc = true;
                return Ok(p);
            }
            "--describe-output-json" => {
                p.print_output_doc_json = true;
                return Ok(p);
            }
            "--describe-output-schema" => {
                p.print_output_doc_schema = true;
                return Ok(p);
            }
            "--version-info" => {
                p.print_version_info = true;
                return Ok(p);
            }
            "-p" | "-v" | "--passthrough" => p.passthrough = true,
            "--quiet-summary" => p.quiet_summary = true,
            "--no-nextest" => p.no_nextest = true,
            "--wrap-unknown" => p.wrap_unknown = true,
            "--absolute-log-paths" => p.absolute_log_paths = true,
            "--timeout" => {
                i += 1;
                let v = raw
                    .get(i)
                    .ok_or_else(|| "--timeout requires a value in seconds".to_string())?;
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--timeout value '{v}' is not an integer"))?;
                p.timeout_secs = Some(n);
            }
            "--heartbeat" | "--heartbeat-secs" => {
                i += 1;
                let v = raw
                    .get(i)
                    .ok_or_else(|| "--heartbeat requires a value in seconds".to_string())?;
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--heartbeat value '{v}' is not an integer"))?;
                p.heartbeat_secs = Some(n);
            }
            "--heartbeat-lines" => {
                i += 1;
                let v = raw
                    .get(i)
                    .ok_or_else(|| "--heartbeat-lines requires a line count".to_string())?;
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--heartbeat-lines value '{v}' is not an integer"))?;
                p.heartbeat_lines = Some(n);
            }
            _ => {
                p.cargo_args = raw[i..].to_vec();
                break;
            }
        }
        i += 1;
    }
    if p.print_help
        || p.print_output_doc
        || p.print_output_doc_json
        || p.print_output_doc_schema
        || p.print_version_info
    {
        return Ok(p);
    }
    if p.cargo_args.is_empty() {
        return Err("usage: cargo-summary [tool-flags] <cargo-subcommand> [cargo args...]".into());
    }
    Ok(p)
}

fn print_help() {
    let help = r"cargo-summary -- concise cargo wrapper

USAGE:
    cargo-summary [tool-flags] <cargo-subcommand> [cargo args...]
    cargo summary  [tool-flags] <cargo-subcommand> [cargo args...]

TOOL FLAGS (must precede the cargo subcommand):
    -p, --passthrough           Relay cargo output verbatim alongside the summary
    -v                          Alias for --passthrough
    --timeout SECS              Kill the child after SECS (exit 124)
    --heartbeat SECS            Emit a liveness line after SECS of silence
    --heartbeat-secs SECS       Alias for --heartbeat
    --heartbeat-lines N         Emit a liveness line after every N output lines
    --no-nextest                Force `cargo test` for test mode (skip nextest)
    --wrap-unknown              Apply wrapper machinery (heartbeat/timeout/logs)
                                for cargo subcommands cargo-summary doesn't know
                                how to summarize. Without this flag, unknown
                                subcommands forward to cargo with inherited stdio.
    --absolute-log-paths        Emit log paths in the summary as absolute paths.
                                By default they are relative to the current
                                working directory.
    --quiet-summary             Suppress the trailing summary line
    --describe-output           Print the output-format grammar and exit
    --describe-output-json      Print the output-format description as JSON
    --describe-output-schema    Print the JSON Schema (draft 2020-12) for the
                                --describe-output-json document and exit
    --version-info              Print cargo-summary, cargo, and nextest versions
    -h, --help                  Print this help and exit

SUMMARIZABLE SUBCOMMANDS:
    build, check, test, clippy

Other cargo subcommands are forwarded raw unless --wrap-unknown is set.

EXAMPLES:
    cargo summary test --workspace
    cargo summary --heartbeat 30 build --release --workspace
    cargo summary --timeout 600 --wrap-unknown doc --workspace
    cargo summary --describe-output           # print the format docs
";
    eprint!("{help}");
}

fn version_info_text() -> String {
    let cargo = cargo_version().map_or_else(
        || "unavailable".to_string(),
        |v| format!("{}.{}.{}", v.major, v.minor, v.patch),
    );
    let nextest = nextest_version().unwrap_or_else(|| "not installed".into());
    format!("cargo-summary {WRAPPER_VERSION}\ncargo: {cargo}\nnextest: {nextest}\n")
}

// ============================================================================
// Raw passthrough
// ============================================================================

fn run_raw(cargo_args: &[String]) -> ExitCode {
    let status = Command::new("cargo")
        .args(cargo_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match status {
        Ok(s) => s
            .code()
            .map_or_else(|| ExitCode::from(1), |c| ExitCode::from(clamp_exit_code(c))),
        Err(e) => {
            eprintln!("cargo-summary: failed to spawn cargo: {e}");
            ExitCode::from(2)
        }
    }
}

fn clamp_exit_code(c: i32) -> u8 {
    // Cargo never exits outside [0, 255] in practice; map anything
    // else to 1 to keep our wrapper's exit code well-typed.
    u8::try_from(c).unwrap_or(1)
}

// ============================================================================
// Wrapped mode pipeline
// ============================================================================

// I/O capture pipeline + heartbeat + timeout + summarization, all
// driven by a single select loop. Splitting into helpers would have
// to thread an awkward amount of mutable state across the boundary.
#[allow(clippy::too_many_lines)]
fn run_wrapped(parsed: &ParsedArgs, kind: Option<SubcommandKind>) -> ExitCode {
    let started = Instant::now();
    let mode = parsed.cargo_args[0].clone();

    let use_nextest = matches!(kind, Some(SubcommandKind::Test))
        && !parsed.no_nextest
        && cargo_nextest_available();

    let (cmd_args, extra_env): (Vec<String>, Vec<(&str, String)>) = if use_nextest {
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
            args,
            vec![("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1".into())],
        )
    } else {
        (parsed.cargo_args.clone(), Vec::new())
    };

    let log_dir = resolve_log_dir();
    let (mut stdout_log, mut stderr_log, stdout_log_path, stderr_log_path) =
        open_log_files(&log_dir, &mode);

    let mut cmd = Command::new("cargo");
    cmd.args(&cmd_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cargo-summary: failed to spawn cargo: {e}");
            return ExitCode::from(2);
        }
    };

    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let tx_o = event_tx.clone();
    let stdout_handle = thread::spawn(move || {
        let r = BufReader::new(stdout);
        for line in r.lines().map_while(Result::ok) {
            if tx_o.send(Event::Stdout(line)).is_err() {
                break;
            }
        }
        let _ = tx_o.send(Event::StdoutEof);
    });
    let tx_e = event_tx.clone();
    let stderr_handle = thread::spawn(move || {
        let r = BufReader::new(stderr);
        for line in r.lines().map_while(Result::ok) {
            if tx_e.send(Event::Stderr(line)).is_err() {
                break;
            }
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

    let select_interval = Duration::from_millis(200);

    loop {
        if stdout_eof && stderr_eof {
            break;
        }
        if let Some(deadline) = parsed.timeout_secs
            && started.elapsed() >= Duration::from_secs(deadline)
        {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        match event_rx.recv_timeout(select_interval) {
            Ok(Event::Stdout(line)) => {
                if parsed.passthrough {
                    let _ = writeln!(std::io::stdout().lock(), "{line}");
                }
                if let Some(f) = stdout_log.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
                if !line.trim().is_empty() {
                    latest_status_line = Some(line.clone());
                }
                stdout_lines.push(line);
                lines_since_heartbeat += 1;
            }
            Ok(Event::Stderr(line)) => {
                if parsed.passthrough {
                    let _ = writeln!(std::io::stderr().lock(), "{line}");
                }
                if let Some(f) = stderr_log.as_mut() {
                    let _ = writeln!(f, "{line}");
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
        let line_trigger = parsed
            .heartbeat_lines
            .is_some_and(|n| lines_since_heartbeat >= n);
        let time_trigger = parsed
            .heartbeat_secs
            .is_some_and(|s| last_heartbeat.elapsed() >= Duration::from_secs(s));
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

    let _ = stdout_handle.join();
    let _ = stderr_handle.join();
    let exit = wait_with_timeout(&mut child, parsed.timeout_secs, started);
    let elapsed = started.elapsed().as_secs_f64();

    let summary: Option<Summary> = if timed_out {
        Some(Summary::Timeout {
            elapsed_secs: elapsed,
            limit_secs: parsed.timeout_secs.unwrap_or(0),
        })
    } else {
        match kind {
            Some(SubcommandKind::Test) if use_nextest => Some(summarize_test_nextest(
                &stdout_lines,
                &stderr_lines,
                exit.success,
                elapsed,
            )),
            Some(SubcommandKind::Test) => Some(summarize_test_legacy(
                &stdout_lines,
                &stderr_lines,
                exit.success,
                elapsed,
            )),
            Some(k @ (SubcommandKind::Build | SubcommandKind::Check)) => {
                Some(summarize_build(&stderr_lines, exit.success, elapsed, k))
            }
            Some(SubcommandKind::Clippy) => {
                Some(summarize_clippy(&stderr_lines, exit.success, elapsed))
            }
            None => None,
        }
    };

    // Discourage unused-import lint flagging TestRunner in the bin.
    let _ = TestRunner::Nextest;

    if !parsed.quiet_summary {
        if let Some(f) = stdout_log.as_mut() {
            let _ = f.flush();
        }
        if let Some(f) = stderr_log.as_mut() {
            let _ = f.flush();
        }
        if let Some(s) = summary {
            let logs = render_logs_tail(
                stdout_log_path.as_deref(),
                stderr_log_path.as_deref(),
                parsed.absolute_log_paths,
            );
            println!("{}{}", s.render(), logs);
        }
    }

    if timed_out {
        return ExitCode::from(124);
    }
    if !exit.success {
        return ExitCode::from(clamp_exit_code(exit.code.unwrap_or(1)));
    }
    ExitCode::SUCCESS
}

fn render_logs_tail(
    stdout_path: Option<&Path>,
    stderr_path: Option<&Path>,
    absolute: bool,
) -> String {
    match (stdout_path, stderr_path) {
        (Some(o), Some(e)) => {
            let o = if absolute {
                o.to_path_buf()
            } else {
                relativize_to_cwd(o)
            };
            let e = if absolute {
                e.to_path_buf()
            } else {
                relativize_to_cwd(e)
            };
            format!(" logs.stdout={} logs.stderr={}", o.display(), e.display())
        }
        _ => String::new(),
    }
}

struct ExitInfo {
    success: bool,
    code: Option<i32>,
}

fn wait_with_timeout(child: &mut Child, timeout_secs: Option<u64>, started: Instant) -> ExitInfo {
    let Some(limit_secs) = timeout_secs else {
        let s = child.wait().expect("wait");
        return ExitInfo {
            success: s.success(),
            code: s.code(),
        };
    };
    let limit = Duration::from_secs(limit_secs);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                return ExitInfo {
                    success: s.success(),
                    code: s.code(),
                };
            }
            Ok(None) => {
                if started.elapsed() >= limit {
                    let _ = child.kill();
                    let s = child.wait().expect("wait after kill");
                    return ExitInfo {
                        success: false,
                        code: s.code(),
                    };
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                return ExitInfo {
                    success: false,
                    code: None,
                };
            }
        }
    }
}

// ============================================================================
// Heartbeat / event types
// ============================================================================

enum Event {
    Stdout(String),
    Stderr(String),
    StdoutEof,
    StderrEof,
}

fn emit_heartbeat(
    started: Instant,
    total_stdout: u64,
    total_stderr: u64,
    lines_since_last: u64,
    latest_line: Option<&str>,
) {
    let elapsed = started.elapsed().as_secs_f64();
    let snippet = latest_line.map(truncate_120).unwrap_or_default();
    let trailer = if snippet.is_empty() {
        String::new()
    } else {
        format!(" latest=\"{snippet}\"")
    };
    eprintln!(
        "[heartbeat {elapsed:.0}s stdout={total_stdout} stderr={total_stderr} +{lines_since_last} since-last{trailer}]"
    );
}

fn truncate_120(s: &str) -> String {
    let mut out = String::new();
    for (count, ch) in s.chars().enumerate() {
        if count >= 120 {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

// ============================================================================
// Diagnostic log files
// ============================================================================

/// Resolve `<package-or-workspace-root>/target/cargo_summary`. Walk
/// up from CWD picking the nearest `Cargo.toml` declaring a
/// `[workspace]`; if none, the nearest one declaring a `[package]`;
/// failing that, just use `<cwd>/target/cargo_summary`.
fn resolve_log_dir() -> PathBuf {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut workspace_root: Option<PathBuf> = None;
    let mut package_root: Option<PathBuf> = None;
    let mut probe = cwd.clone();
    loop {
        let candidate = probe.join("Cargo.toml");
        if candidate.exists()
            && let Ok(text) = std::fs::read_to_string(&candidate)
        {
            if text.contains("[workspace]") && workspace_root.is_none() {
                workspace_root = Some(probe.clone());
            }
            if text.contains("[package]") && package_root.is_none() {
                package_root = Some(probe.clone());
            }
        }
        if !probe.pop() {
            break;
        }
    }
    let root = workspace_root.or(package_root).unwrap_or(cwd);
    root.join("target").join("cargo_summary")
}

fn open_log_files(
    log_dir: &Path,
    mode: &str,
) -> (Option<File>, Option<File>, Option<PathBuf>, Option<PathBuf>) {
    if std::fs::create_dir_all(log_dir).is_err() {
        return (None, None, None, None);
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let stdout_path = log_dir.join(format!("{mode}-{ts}.stdout.log"));
    let stderr_path = log_dir.join(format!("{mode}-{ts}.stderr.log"));
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

// ============================================================================
// Tests (binary-local; lib-side tests live in src/lib.rs)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn s(strs: &[&str]) -> Vec<String> {
        strs.iter().map(std::string::ToString::to_string).collect()
    }

    // ---- CLI parsing ----

    #[test]
    fn parse_args_strips_summary_token_handled_in_main() {
        let p = parse_args(&s(&["build"])).unwrap();
        assert_eq!(p.cargo_args, vec!["build"]);
    }

    #[test]
    fn parse_args_accepts_wrap_unknown() {
        let p = parse_args(&s(&["--wrap-unknown", "doc"])).unwrap();
        assert!(p.wrap_unknown);
        assert_eq!(p.cargo_args, vec!["doc"]);
    }

    #[test]
    fn parse_args_help_short_circuits() {
        let p = parse_args(&s(&["--help"])).unwrap();
        assert!(p.print_help);
        assert!(p.cargo_args.is_empty());
    }

    #[test]
    fn parse_args_describe_output_short_circuits() {
        assert!(
            parse_args(&s(&["--describe-output"]))
                .unwrap()
                .print_output_doc
        );
        assert!(
            parse_args(&s(&["--describe-output-json"]))
                .unwrap()
                .print_output_doc_json
        );
        assert!(
            parse_args(&s(&["--describe-output-schema"]))
                .unwrap()
                .print_output_doc_schema
        );
    }

    #[test]
    fn parse_args_requires_subcommand() {
        assert!(parse_args(&s(&[])).is_err());
        assert!(parse_args(&s(&["--passthrough"])).is_err());
    }

    #[test]
    fn parse_args_rejects_bad_int() {
        let err = parse_args(&s(&["--timeout", "ten", "build"])).unwrap_err();
        assert!(err.contains("not an integer"));
    }

    #[test]
    fn parse_args_accepts_zero_timeout() {
        // Zero is a legitimate (if degenerate) value; should parse cleanly.
        let p = parse_args(&s(&["--timeout", "0", "build"])).unwrap();
        assert_eq!(p.timeout_secs, Some(0));
    }

    #[test]
    fn parse_args_missing_value_errors() {
        let err = parse_args(&s(&["--timeout"])).unwrap_err();
        assert!(err.contains("--timeout requires"), "got: {err}");
        let err = parse_args(&s(&["--heartbeat"])).unwrap_err();
        assert!(err.contains("--heartbeat requires"), "got: {err}");
        let err = parse_args(&s(&["--heartbeat-lines"])).unwrap_err();
        assert!(err.contains("--heartbeat-lines requires"), "got: {err}");
    }

    #[test]
    fn parse_args_unknown_first_token_treated_as_subcommand() {
        // An unrecognized leading token is the cargo subcommand. The
        // wrap/raw routing happens in main; the parser is happy.
        let p = parse_args(&s(&["run", "--release"])).unwrap();
        assert_eq!(p.cargo_args, vec!["run", "--release"]);
    }

    #[test]
    fn parse_args_propagates_cargo_args_after_subcommand() {
        let p = parse_args(&s(&[
            "--timeout",
            "60",
            "--heartbeat",
            "5",
            "test",
            "--workspace",
            "--release",
            "my_test",
        ]))
        .unwrap();
        assert_eq!(p.timeout_secs, Some(60));
        assert_eq!(p.heartbeat_secs, Some(5));
        assert_eq!(
            p.cargo_args,
            vec!["test", "--workspace", "--release", "my_test"]
        );
    }

    #[test]
    fn parse_args_passthrough_aliases() {
        for token in ["-p", "-v", "--passthrough"] {
            let p = parse_args(&s(&[token, "build"])).unwrap();
            assert!(p.passthrough, "{token} did not enable passthrough");
        }
    }

    // ---- Path / log tail rendering ----

    #[test]
    fn render_logs_tail_uses_relative_by_default() {
        let cwd = env::current_dir().unwrap();
        let stdout = cwd
            .join("target")
            .join("cargo_summary")
            .join("build-1.stdout.log");
        let stderr = cwd
            .join("target")
            .join("cargo_summary")
            .join("build-1.stderr.log");
        let tail = render_logs_tail(Some(&stdout), Some(&stderr), false);
        assert!(tail.starts_with(" logs.stdout="));
        assert!(
            !tail.contains(&*cwd.to_string_lossy()),
            "relative tail must not include CWD: {tail}"
        );
    }

    #[test]
    fn render_logs_tail_uses_absolute_when_flagged() {
        let cwd = env::current_dir().unwrap();
        let stdout = cwd.join("target").join("x.log");
        let stderr = cwd.join("target").join("y.log");
        let tail = render_logs_tail(Some(&stdout), Some(&stderr), true);
        assert!(
            tail.contains(&*cwd.to_string_lossy()),
            "absolute tail must include CWD: {tail}"
        );
    }

    #[test]
    fn render_logs_tail_empty_when_paths_missing() {
        assert_eq!(render_logs_tail(None, None, false), "");
        assert_eq!(render_logs_tail(None, Some(Path::new("x")), false), "");
        assert_eq!(render_logs_tail(Some(Path::new("x")), None, false), "");
    }

    // ---- Exit code clamping ----

    #[test]
    fn clamp_exit_code_in_range() {
        assert_eq!(clamp_exit_code(0), 0);
        assert_eq!(clamp_exit_code(124), 124);
        assert_eq!(clamp_exit_code(255), 255);
    }

    #[test]
    fn clamp_exit_code_out_of_range_falls_back_to_one() {
        assert_eq!(clamp_exit_code(-1), 1);
        assert_eq!(clamp_exit_code(256), 1);
        assert_eq!(clamp_exit_code(i32::MIN), 1);
        assert_eq!(clamp_exit_code(i32::MAX), 1);
    }

    // ---- Heartbeat snippet truncation ----

    #[test]
    fn truncate_120_passthrough_short() {
        assert_eq!(truncate_120("hello"), "hello");
    }

    #[test]
    fn truncate_120_uses_ascii_ellipsis_on_long_input() {
        let long: String = "x".repeat(200);
        let t = truncate_120(&long);
        assert!(t.ends_with("..."));
        assert!(!t.contains('\u{2026}'));
        assert_eq!(t.chars().count(), 123);
    }

    #[test]
    fn truncate_120_handles_multibyte_chars() {
        // Char count, not byte count, is the cap.
        let s: String = "ñ".repeat(200);
        let t = truncate_120(&s);
        assert!(t.ends_with("..."));
        // 120 chars of "ñ" + "..." = 123 chars total.
        assert_eq!(t.chars().count(), 123);
    }

    #[test]
    fn truncate_120_at_exact_boundary() {
        let s: String = "a".repeat(120);
        assert_eq!(truncate_120(&s), s);
    }
}
