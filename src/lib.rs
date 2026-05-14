// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Library half of `cargo-summary`. Exposes the typed summary
//! representation, the parsers that build summaries from cargo /
//! nextest output, the cargo / nextest version detection helpers,
//! the self-describing output documentation, and the path
//! relativization utility used by the CLI.
//!
//! Downstream tools that want to consume cargo-summary lines can
//! depend on the library and build their own pipelines without
//! re-parsing the text form. The CLI binary in `src/bin` is a thin
//! orchestration layer over these primitives.
//!
//! ## Versioning
//!
//! Pre-1.0. The public API may change between minor versions; pin
//! exact patch versions in `Cargo.toml` if you depend on it. The
//! emitted text format is documented by [`output_doc_text`] /
//! [`output_doc_json`] and identified by the URN in
//! [`OUTPUT_DOC_SCHEMA_URN`].

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

// ============================================================================
// Subcommand classification
// ============================================================================

/// One of the cargo subcommands cargo-summary explicitly parses.
///
/// Use [`SubcommandKind::for_name`] to map a string token to its
/// variant; unrecognized subcommands return `None` (cargo-summary
/// forwards those raw rather than summarizing them).
///
/// ```
/// use cargo_summary::SubcommandKind;
/// assert_eq!(SubcommandKind::for_name("test"), Some(SubcommandKind::Test));
/// assert_eq!(SubcommandKind::for_name("doc"),  None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubcommandKind {
    Build,
    Check,
    Test,
    Clippy,
}

impl SubcommandKind {
    /// All summarizable subcommand names in canonical order. Useful
    /// for help text and error messages.
    pub const ALL_NAMES: &'static [&'static str] = &["build", "check", "test", "clippy"];

    /// Map a cargo subcommand name to its `SubcommandKind`. Returns
    /// `None` for any subcommand cargo-summary does not summarize.
    pub fn for_name(name: &str) -> Option<Self> {
        match name {
            "build" => Some(Self::Build),
            "check" => Some(Self::Check),
            "test" => Some(Self::Test),
            "clippy" => Some(Self::Clippy),
            _ => None,
        }
    }

    /// The all-caps label used in summary lines (`BUILD`, `TEST`,
    /// etc.).
    pub fn mode_label(&self) -> &'static str {
        match self {
            Self::Build => "BUILD",
            Self::Check => "CHECK",
            Self::Test => "TEST",
            Self::Clippy => "CLIPPY",
        }
    }
}

// ============================================================================
// Cargo / nextest version detection
// ============================================================================

/// Parsed semver-shaped cargo version (major.minor.patch). Pre-release
/// and build suffixes are dropped.
///
/// ```
/// use cargo_summary::CargoVersion;
/// let v = CargoVersion::parse("cargo 1.94.1 (29ea6fb6a 2026-03-24)").unwrap();
/// assert_eq!((v.major, v.minor, v.patch), (1, 94, 1));
///
/// let nightly = CargoVersion::parse("cargo 1.95.0-nightly (abc 2026-04-01)").unwrap();
/// assert_eq!(nightly.minor, 95);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CargoVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl CargoVersion {
    /// Parse the first line of `cargo --version` output. Returns
    /// `None` on any malformed input.
    pub fn parse(s: &str) -> Option<Self> {
        let token = s.split_whitespace().nth(1)?;
        let core = token.split('-').next()?;
        let mut parts = core.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        let patch: u32 = parts.next().unwrap_or("0").parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

/// Detect the installed cargo's version by invoking `cargo --version`.
/// Cached after the first successful detection; subsequent calls are
/// free. Returns `None` if cargo is missing or its output is unparseable.
pub fn cargo_version() -> Option<CargoVersion> {
    static CACHE: OnceLock<Option<CargoVersion>> = OnceLock::new();
    *CACHE.get_or_init(|| {
        let out = Command::new("cargo")
            .arg("--version")
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8(out.stdout).ok()?;
        CargoVersion::parse(stdout.trim())
    })
}

/// Detect the installed `cargo-nextest`'s version banner by invoking
/// `cargo nextest --version`. Only the first line is retained.
/// Cached after first use.
pub fn nextest_version() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let out = Command::new("cargo")
                .args(["nextest", "--version"])
                .stderr(Stdio::null())
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let s = String::from_utf8(out.stdout).ok()?;
            let first = s.lines().next().unwrap_or("").trim().to_string();
            if first.is_empty() { None } else { Some(first) }
        })
        .clone()
}

/// `true` iff `cargo-nextest` is callable. Used by the CLI to decide
/// whether to route the `test` subcommand through the nextest JSON
/// path.
pub fn cargo_nextest_available() -> bool {
    nextest_version().is_some()
}

// ============================================================================
// Summary type
// ============================================================================

/// Canonical representation of the one line cargo-summary emits per
/// invocation. Every shape is covered by a variant; rendering goes
/// through [`Summary::render`].
///
/// ```
/// use cargo_summary::{Summary, SubcommandKind, TestRunner};
///
/// let s = Summary::BuildOk {
///     mode: SubcommandKind::Build,
///     warnings: 0,
///     elapsed_secs: 12.3,
/// };
/// assert_eq!(s.render(), "BUILD OK in 12.3s");
///
/// let s = Summary::TestFailed {
///     runner: TestRunner::Nextest,
///     total: 12, passed: 11, failed: 1, ignored: 0,
///     failures: vec!["my::failing_test".into()],
///     elapsed_secs: 2.5,
/// };
/// assert!(s.render().contains("failures=[my::failing_test]"));
/// assert!(s.render().ends_with("[nextest]"));
/// ```
#[derive(Clone, Debug)]
pub enum Summary {
    BuildOk {
        mode: SubcommandKind,
        warnings: u64,
        elapsed_secs: f64,
    },
    BuildFailed {
        mode: SubcommandKind,
        errors: Vec<String>,
        elapsed_secs: f64,
    },
    TestOk {
        runner: TestRunner,
        total: u64,
        passed: u64,
        failed: u64,
        ignored: u64,
        elapsed_secs: f64,
    },
    TestFailed {
        runner: TestRunner,
        total: u64,
        passed: u64,
        failed: u64,
        ignored: u64,
        failures: Vec<String>,
        elapsed_secs: f64,
    },
    TestBuildFailed {
        runner: TestRunner,
        errors: Vec<String>,
        elapsed_secs: f64,
    },
    ClippyOk {
        warnings: u64,
        elapsed_secs: f64,
    },
    ClippyFailed {
        warnings: u64,
        diagnostics: Vec<String>,
        elapsed_secs: f64,
    },
    Timeout {
        elapsed_secs: f64,
        limit_secs: u64,
    },
}

/// Which test runner produced a test summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestRunner {
    Nextest,
    Legacy,
}

impl TestRunner {
    /// The short tag that appears at the end of test summary lines
    /// (`[nextest]` or `[legacy]`).
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Nextest => "nextest",
            Self::Legacy => "legacy",
        }
    }
}

impl Summary {
    /// Render this summary as the single line cargo-summary emits.
    /// The `logs.stdout=` / `logs.stderr=` suffix is appended by the
    /// CLI after this call; this function returns only the body.
    pub fn render(&self) -> String {
        match self {
            Self::BuildOk {
                mode,
                warnings,
                elapsed_secs,
            } => {
                let warns = if *warnings > 0 {
                    format!(" warnings={}", warnings)
                } else {
                    String::new()
                };
                format!("{} OK{} in {:.1}s", mode.mode_label(), warns, elapsed_secs)
            }
            Self::BuildFailed {
                mode,
                errors,
                elapsed_secs,
            } => {
                if errors.is_empty() {
                    format!(
                        "{} FAILED in {:.1}s [no error lines captured]",
                        mode.mode_label(),
                        elapsed_secs
                    )
                } else {
                    format!(
                        "{} FAILED in {:.1}s [{}]",
                        mode.mode_label(),
                        elapsed_secs,
                        errors.join(" | ")
                    )
                }
            }
            Self::TestOk {
                runner,
                total,
                passed,
                failed,
                ignored,
                elapsed_secs,
            } => format!(
                "TEST OK total={} passed={} failed={} ignored={} in {:.1}s [{}]",
                total,
                passed,
                failed,
                ignored,
                elapsed_secs,
                runner.tag()
            ),
            Self::TestFailed {
                runner,
                total,
                passed,
                failed,
                ignored,
                failures,
                elapsed_secs,
            } => {
                let fail_list = if failures.is_empty() {
                    String::new()
                } else {
                    format!(" failures=[{}]", failures.join(", "))
                };
                format!(
                    "TEST FAILED total={} passed={} failed={} ignored={}{} in {:.1}s [{}]",
                    total,
                    passed,
                    failed,
                    ignored,
                    fail_list,
                    elapsed_secs,
                    runner.tag()
                )
            }
            Self::TestBuildFailed {
                runner,
                errors,
                elapsed_secs,
            } => {
                let body = if errors.is_empty() {
                    "no error lines captured".to_string()
                } else {
                    errors.join(" | ")
                };
                format!(
                    "TEST BUILD-FAILED in {:.1}s [{}] [{}]",
                    elapsed_secs,
                    body,
                    runner.tag()
                )
            }
            Self::ClippyOk {
                warnings,
                elapsed_secs,
            } => format!("CLIPPY OK warnings={} in {:.1}s", warnings, elapsed_secs),
            Self::ClippyFailed {
                warnings,
                diagnostics,
                elapsed_secs,
            } => {
                let body = if diagnostics.is_empty() {
                    "no diagnostic lines captured".to_string()
                } else {
                    diagnostics.join(" | ")
                };
                format!(
                    "CLIPPY FAILED warnings={} in {:.1}s [{}]",
                    warnings, elapsed_secs, body
                )
            }
            Self::Timeout {
                elapsed_secs,
                limit_secs,
            } => format!(
                "TIMEOUT after {:.1}s (limit {}s) -- child killed",
                elapsed_secs, limit_secs
            ),
        }
    }
}

// ============================================================================
// Summarizers (parse cargo / nextest output, produce Summary)
// ============================================================================

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

/// Cap on the number of failure names captured in `TestFailed`.
pub const MAX_FAILURE_NAMES: usize = 8;

/// Cap on the number of diagnostic lines captured in `BuildFailed`,
/// `TestBuildFailed`, and `ClippyFailed`.
pub const MAX_DIAGNOSTIC_LINES: usize = 6;

/// Build a `Summary` from a captured `cargo nextest run` invocation.
///
/// `stdout` is the line-delimited libtest-json event stream; `stderr`
/// is cargo's stderr (used for compile-error capture when the build
/// failed before any test ran). `ok` is the child's success flag,
/// `secs` is the elapsed wall-clock time.
///
/// ```
/// use cargo_summary::summarize_test_nextest;
///
/// let stdout: Vec<String> = vec![
///     r#"{"type":"suite","event":"ok","passed":3,"failed":0,"ignored":1}"#.into(),
/// ];
/// let out = summarize_test_nextest(&stdout, &[], true, 0.5);
/// assert_eq!(out.render(), "TEST OK total=4 passed=3 failed=0 ignored=1 in 0.5s [nextest]");
/// ```
pub fn summarize_test_nextest(
    stdout: &[String],
    stderr: &[String],
    ok: bool,
    secs: f64,
) -> Summary {
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut ignored = 0u64;
    let mut suite_seen = false;
    let mut failure_names: Vec<String> = Vec::new();
    let mut compile_errors: Vec<String> = Vec::new();

    for line in stdout {
        if !line.starts_with('{') {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<NextestEvent>(line) {
            match ev {
                NextestEvent::Suite(s) if s.event == "ok" || s.event == "failed" => {
                    passed += s.passed;
                    failed += s.failed;
                    ignored += s.ignored;
                    suite_seen = true;
                }
                NextestEvent::Test(t) if t.event == "failed" => {
                    if failure_names.len() < MAX_FAILURE_NAMES {
                        let name = t.name.split('$').next_back().unwrap_or(&t.name).to_string();
                        failure_names.push(name);
                    }
                }
                _ => {}
            }
        }
    }

    for line in stderr {
        if line.starts_with("error[") || line.starts_with("error: ") {
            compile_errors.push(line.clone());
            if compile_errors.len() >= MAX_DIAGNOSTIC_LINES {
                break;
            }
        }
    }
    if !compile_errors.is_empty() && !suite_seen {
        return Summary::TestBuildFailed {
            runner: TestRunner::Nextest,
            errors: compile_errors,
            elapsed_secs: secs,
        };
    }

    let total = passed + failed + ignored;
    if ok && failed == 0 {
        Summary::TestOk {
            runner: TestRunner::Nextest,
            total,
            passed,
            failed,
            ignored,
            elapsed_secs: secs,
        }
    } else {
        Summary::TestFailed {
            runner: TestRunner::Nextest,
            total,
            passed,
            failed,
            ignored,
            failures: failure_names,
            elapsed_secs: secs,
        }
    }
}

/// Build a `Summary` from a captured `cargo test` (libtest) run.
///
/// Parses `test result: ...` lines for counts and `test NAME ... FAILED`
/// lines for failure names. See also [`summarize_test_nextest`] for
/// the JSON path.
///
/// ```
/// use cargo_summary::summarize_test_legacy;
///
/// let stdout = vec![
///     "test result: ok. 5 passed; 0 failed; 1 ignored; 0 measured".to_string(),
/// ];
/// let out = summarize_test_legacy(&stdout, &[], true, 1.0);
/// assert_eq!(out.render(), "TEST OK total=6 passed=5 failed=0 ignored=1 in 1.0s [legacy]");
/// ```
pub fn summarize_test_legacy(stdout: &[String], stderr: &[String], ok: bool, secs: f64) -> Summary {
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
        if let Some(rest) = line.strip_prefix("test ")
            && rest.contains(" ... FAILED")
        {
            let name = rest.split(" ... FAILED").next().unwrap_or("").to_string();
            if failure_names.len() < MAX_FAILURE_NAMES {
                failure_names.push(name);
            }
        }
    }

    for line in stderr {
        if line.starts_with("error[") || line.starts_with("error: ") {
            compile_errors.push(line.clone());
            if compile_errors.len() >= MAX_DIAGNOSTIC_LINES {
                break;
            }
        }
    }

    if !compile_errors.is_empty() {
        return Summary::TestBuildFailed {
            runner: TestRunner::Legacy,
            errors: compile_errors,
            elapsed_secs: secs,
        };
    }

    let total = passed + failed + ignored;
    if ok && failed == 0 {
        Summary::TestOk {
            runner: TestRunner::Legacy,
            total,
            passed,
            failed,
            ignored,
            elapsed_secs: secs,
        }
    } else {
        Summary::TestFailed {
            runner: TestRunner::Legacy,
            total,
            passed,
            failed,
            ignored,
            failures: failure_names,
            elapsed_secs: secs,
        }
    }
}

/// Build a `Summary` for `cargo build` or `cargo check`. The `kind`
/// determines the line's leading label.
///
/// ```
/// use cargo_summary::{summarize_build, SubcommandKind};
///
/// let stderr = vec![
///     "warning: unused import".to_string(),
///     "warning: dead code".to_string(),
/// ];
/// let out = summarize_build(&stderr, true, 2.5, SubcommandKind::Build);
/// assert_eq!(out.render(), "BUILD OK warnings=2 in 2.5s");
/// ```
pub fn summarize_build(stderr: &[String], ok: bool, secs: f64, kind: SubcommandKind) -> Summary {
    if ok {
        let warnings = stderr.iter().filter(|l| l.starts_with("warning:")).count() as u64;
        return Summary::BuildOk {
            mode: kind,
            warnings,
            elapsed_secs: secs,
        };
    }
    let errors: Vec<String> = stderr
        .iter()
        .filter(|l| l.starts_with("error[") || l.starts_with("error: "))
        .take(MAX_DIAGNOSTIC_LINES)
        .cloned()
        .collect();
    Summary::BuildFailed {
        mode: kind,
        errors,
        elapsed_secs: secs,
    }
}

/// Build a `Summary` for `cargo clippy`.
///
/// The warning count is always emitted (even at zero) to distinguish
/// `CLIPPY OK` from `BUILD OK`.
///
/// ```
/// use cargo_summary::summarize_clippy;
/// let out = summarize_clippy(&[], true, 0.7);
/// assert_eq!(out.render(), "CLIPPY OK warnings=0 in 0.7s");
/// ```
pub fn summarize_clippy(stderr: &[String], ok: bool, secs: f64) -> Summary {
    let warn_count = stderr.iter().filter(|l| l.starts_with("warning:")).count() as u64;
    if ok {
        return Summary::ClippyOk {
            warnings: warn_count,
            elapsed_secs: secs,
        };
    }
    let mut diags: Vec<String> = Vec::new();
    for l in stderr
        .iter()
        .filter(|l| l.starts_with("error[") || l.starts_with("error: "))
        .take(MAX_DIAGNOSTIC_LINES)
    {
        diags.push(l.clone());
    }
    if diags.is_empty() {
        for l in stderr
            .iter()
            .filter(|l| l.starts_with("warning:"))
            .take(MAX_DIAGNOSTIC_LINES)
        {
            diags.push(l.clone());
        }
    }
    Summary::ClippyFailed {
        warnings: warn_count,
        diagnostics: diags,
        elapsed_secs: secs,
    }
}

// ============================================================================
// Path relativization
// ============================================================================

/// Make `path` relative to the current working directory, if
/// possible. Falls back to the input unchanged when the CWD is not
/// available or the two paths cannot share a root (e.g. different
/// Windows drives).
pub fn relativize_to_cwd(path: &Path) -> PathBuf {
    let Ok(cwd) = std::env::current_dir() else {
        return path.to_path_buf();
    };
    relativize_against(path, &cwd)
}

/// Compute `path` relative to `base`. When the target lives under
/// the base, the result is a subpath; otherwise the result uses
/// `..` to ascend out of `base` to a shared ancestor. Paths with no
/// shared root anchor are returned unchanged.
///
/// ```
/// use cargo_summary::relativize_against;
/// use std::path::{Path, PathBuf};
///
/// // Target under base -> subpath.
/// assert_eq!(
///     relativize_against(Path::new("/a/b/c.log"), Path::new("/a")),
///     PathBuf::from("b/c.log"),
/// );
///
/// // Target outside base -> dot-dot prefix.
/// assert_eq!(
///     relativize_against(Path::new("/a/target/x.log"), Path::new("/a/src/bin")),
///     PathBuf::from("../../target/x.log"),
/// );
///
/// // Equal paths -> ".".
/// let p = Path::new("/a/b");
/// assert_eq!(relativize_against(p, p), PathBuf::from("."));
/// ```
pub fn relativize_against(path: &Path, base: &Path) -> PathBuf {
    if let Ok(rel) = path.strip_prefix(base) {
        return if rel.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            rel.to_path_buf()
        };
    }
    let base_comps: Vec<Component> = base.components().collect();
    let path_comps: Vec<Component> = path.components().collect();
    let base_root = base_comps.first().map(|c| c.as_os_str());
    let path_root = path_comps.first().map(|c| c.as_os_str());
    if base_root != path_root {
        return path.to_path_buf();
    }
    let common = base_comps
        .iter()
        .zip(path_comps.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel = PathBuf::new();
    for _ in common..base_comps.len() {
        rel.push("..");
    }
    for c in &path_comps[common..] {
        rel.push(c.as_os_str());
    }
    if rel.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        rel
    }
}

// ============================================================================
// Output documentation (the --describe-output / --describe-output-json data)
// ============================================================================

/// Description of one summary line shape. See [`output_doc_lines`].
#[derive(serde::Serialize)]
pub struct LineDoc {
    pub kind: &'static str,
    pub label: &'static str,
    pub grammar: &'static str,
    pub notes: &'static str,
    pub fields: Vec<FieldDoc>,
    pub examples: Vec<String>,
}

/// Description of one field within a [`LineDoc`].
#[derive(serde::Serialize)]
pub struct FieldDoc {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub optional: bool,
    pub description: &'static str,
}

/// Description of one summarizable cargo subcommand.
#[derive(serde::Serialize)]
pub struct SubcommandDoc {
    pub name: &'static str,
    pub cargo_invocation: &'static str,
    pub summary_kinds: &'static [&'static str],
    pub notes: &'static str,
}

/// Description of how log capture is encoded in summary lines.
#[derive(serde::Serialize)]
pub struct LogCaptureDoc {
    pub description: &'static str,
    pub suffix_grammar: &'static str,
    pub file_layout: &'static str,
}

/// Top-level document emitted by `--describe-output-json`.
#[derive(serde::Serialize)]
pub struct OutputDoc {
    pub schema: &'static str,
    pub doc_version: &'static str,
    pub wrapper_version: &'static str,
    pub subcommands: Vec<SubcommandDoc>,
    pub log_capture: LogCaptureDoc,
    pub lines: Vec<LineDoc>,
}

/// Output-documentation structure version. Bumped on breaking
/// changes to the document shape.
pub const OUTPUT_DOC_VERSION: &str = "1";

/// Stable identifier for the JSON Schema describing
/// `--describe-output-json` output.
pub const OUTPUT_DOC_SCHEMA_URN: &str = "urn:cargo-summary:output-doc-schema:v1";

/// JSON Schema (draft 2020-12) for the `--describe-output-json`
/// document. Embedded as a string so the schema travels with the
/// binary.
pub const OUTPUT_DOC_SCHEMA: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:cargo-summary:output-doc-schema:v1",
  "title": "cargo-summary output description",
  "description": "Schema for the JSON document emitted by `cargo-summary --describe-output-json`. Describes the human-readable summary lines that cargo-summary emits for build/check/test/clippy invocations.",
  "type": "object",
  "required": ["schema", "doc_version", "wrapper_version", "subcommands", "log_capture", "lines"],
  "additionalProperties": false,
  "properties": {
    "schema": {
      "type": "string",
      "description": "URN identifying the schema this document conforms to. Fetch via `cargo-summary --describe-output-schema`."
    },
    "doc_version": {
      "type": "string",
      "description": "Version of the output-description structure; bumped on breaking changes."
    },
    "wrapper_version": {
      "type": "string",
      "description": "Version of the cargo-summary binary that produced this document."
    },
    "subcommands": {
      "type": "array",
      "description": "Cargo subcommands cargo-summary parses and summarizes. All others are forwarded raw unless --wrap-unknown is set.",
      "items": { "$ref": "#/$defs/subcommand" }
    },
    "log_capture": {
      "type": "object",
      "required": ["description", "suffix_grammar", "file_layout"],
      "additionalProperties": false,
      "properties": {
        "description":    { "type": "string" },
        "suffix_grammar": { "type": "string" },
        "file_layout":    { "type": "string" }
      }
    },
    "lines": {
      "type": "array",
      "description": "One entry per summary line shape cargo-summary may emit.",
      "items": { "$ref": "#/$defs/line" }
    }
  },
  "$defs": {
    "subcommand": {
      "type": "object",
      "required": ["name", "cargo_invocation", "summary_kinds", "notes"],
      "additionalProperties": false,
      "properties": {
        "name":             { "type": "string", "description": "Cargo subcommand cargo-summary recognizes." },
        "cargo_invocation": { "type": "string", "description": "Exact cargo invocation used. May list alternatives separated by ' | '." },
        "summary_kinds":    { "type": "array", "items": { "type": "string" }, "description": "Kind identifiers (see lines[].kind) this subcommand may produce." },
        "notes":            { "type": "string" }
      }
    },
    "line": {
      "type": "object",
      "required": ["kind", "label", "grammar", "notes", "fields", "examples"],
      "additionalProperties": false,
      "properties": {
        "kind":     { "type": "string", "description": "Stable snake_case identifier for this line shape." },
        "label":    { "type": "string", "description": "Human-readable label." },
        "grammar":  { "type": "string", "description": "Production-rule-style grammar." },
        "notes":    { "type": "string", "description": "Free-form notes; may be empty." },
        "fields":   { "type": "array", "items": { "$ref": "#/$defs/field" } },
        "examples": { "type": "array", "items": { "type": "string", "description": "Concrete example, produced by the live renderer." } }
      }
    },
    "field": {
      "type": "object",
      "required": ["name", "type", "optional", "description"],
      "additionalProperties": false,
      "properties": {
        "name":        { "type": "string" },
        "type":        { "type": "string", "description": "Informal type label (u64, f64, enum, list<string>, ...)." },
        "optional":    { "type": "boolean" },
        "description": { "type": "string" }
      }
    }
  }
}
"##;

/// Per-subcommand documentation, including the exact cargo
/// invocation cargo-summary runs and which summary kinds it can
/// produce.
pub fn subcommand_docs() -> Vec<SubcommandDoc> {
    vec![
        SubcommandDoc {
            name: "build",
            cargo_invocation: "cargo build [args...]",
            summary_kinds: &["build_ok", "build_failed", "timeout"],
            notes: "Warning count comes from stderr lines starting with `warning:`. Error capture takes up to 6 stderr lines starting with `error[` or `error: `.",
        },
        SubcommandDoc {
            name: "check",
            cargo_invocation: "cargo check [args...]",
            summary_kinds: &["build_ok", "build_failed", "timeout"],
            notes: "Identical parsing to `build`; emits the CHECK mode label instead of BUILD.",
        },
        SubcommandDoc {
            name: "test",
            cargo_invocation: "cargo nextest run --message-format libtest-json --message-format-version 0.1 --no-fail-fast [args...] | cargo test [args...]",
            summary_kinds: &["test_ok", "test_failed", "test_build_failed", "timeout"],
            notes: "When `cargo-nextest` is installed and `--no-nextest` is not set, the nextest path is used: counts come from `suite` events with `event=ok|failed`, and failure names come from `test` events with `event=failed`. Otherwise the legacy path parses libtest's `test result: ...` summary line and `test NAME ... FAILED` lines. Failure name list capped at 8. Either path yields a TEST BUILD-FAILED summary when cargo fails to compile before any test runs.",
        },
        SubcommandDoc {
            name: "clippy",
            cargo_invocation: "cargo clippy [args...]",
            summary_kinds: &["clippy_ok", "clippy_failed", "timeout"],
            notes: "Warning count is always emitted (even at zero) to distinguish from BUILD OK. On failure, the diagnostics list prefers up to 6 `error[`/`error: ` lines, falling back to `warning:` lines if no errors were captured.",
        },
    ]
}

/// Log-capture metadata for the output document.
pub fn log_capture_doc() -> LogCaptureDoc {
    LogCaptureDoc {
        description: "When the wrapper can create log files, every summary line ends with a logs suffix pointing at the captured stdout/stderr. Omitted on failure to create files. Paths are rendered relative to the process CWD by default; pass --absolute-log-paths for absolute paths.",
        suffix_grammar: " logs.stdout=<cwd-relative-path> logs.stderr=<cwd-relative-path>",
        file_layout: "<workspace-or-package-root>/target/cargo_summary/<mode>-<millis-since-epoch>.{stdout,stderr}.log",
    }
}

/// Documentation entries for every summary line shape. Each entry's
/// examples are produced by [`Summary::render`], so they cannot
/// drift from the live renderer.
pub fn output_doc_lines() -> Vec<LineDoc> {
    vec![
        LineDoc {
            kind: "build_ok",
            label: "BUILD OK / CHECK OK",
            grammar: "<MODE> OK [warnings=<N>] in <SECS>s [logs.stdout=<P> logs.stderr=<P>]",
            notes: "MODE is BUILD or CHECK. warnings is emitted only when >0.",
            fields: vec![
                FieldDoc {
                    name: "mode",
                    ty: "enum",
                    optional: false,
                    description: "BUILD or CHECK.",
                },
                FieldDoc {
                    name: "warnings",
                    ty: "u64",
                    optional: true,
                    description: "Count of `warning:` lines on cargo stderr.",
                },
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Wall-clock seconds, one decimal.",
                },
            ],
            examples: vec![
                Summary::BuildOk {
                    mode: SubcommandKind::Build,
                    warnings: 0,
                    elapsed_secs: 12.3,
                }
                .render(),
                Summary::BuildOk {
                    mode: SubcommandKind::Check,
                    warnings: 2,
                    elapsed_secs: 4.1,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "build_failed",
            label: "BUILD FAILED / CHECK FAILED",
            grammar: "<MODE> FAILED in <SECS>s [<diag1> | <diag2> | ...]",
            notes: "Diagnostics list capped at 6, joined with ' | '. Empty list yields '[no error lines captured]'.",
            fields: vec![
                FieldDoc {
                    name: "mode",
                    ty: "enum",
                    optional: false,
                    description: "BUILD or CHECK.",
                },
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Wall-clock seconds.",
                },
                FieldDoc {
                    name: "diagnostics",
                    ty: "list<string>",
                    optional: false,
                    description: "Up to 6 cargo stderr lines starting with `error[` or `error: `.",
                },
            ],
            examples: vec![
                Summary::BuildFailed {
                    mode: SubcommandKind::Build,
                    errors: vec!["error[E0599]: no method named `foo` found".into()],
                    elapsed_secs: 6.2,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "test_ok",
            label: "TEST OK",
            grammar: "TEST OK total=<N> passed=<N> failed=<N> ignored=<N> in <SECS>s [<runner>]",
            notes: "runner is `nextest` or `legacy` depending on the test path used.",
            fields: vec![
                FieldDoc {
                    name: "total",
                    ty: "u64",
                    optional: false,
                    description: "passed + failed + ignored.",
                },
                FieldDoc {
                    name: "passed",
                    ty: "u64",
                    optional: false,
                    description: "",
                },
                FieldDoc {
                    name: "failed",
                    ty: "u64",
                    optional: false,
                    description: "Always 0 for TEST OK.",
                },
                FieldDoc {
                    name: "ignored",
                    ty: "u64",
                    optional: false,
                    description: "",
                },
                FieldDoc {
                    name: "runner",
                    ty: "enum",
                    optional: false,
                    description: "`nextest` or `legacy`.",
                },
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Wall-clock seconds.",
                },
            ],
            examples: vec![
                Summary::TestOk {
                    runner: TestRunner::Nextest,
                    total: 2092,
                    passed: 2081,
                    failed: 0,
                    ignored: 11,
                    elapsed_secs: 18.4,
                }
                .render(),
                Summary::TestOk {
                    runner: TestRunner::Legacy,
                    total: 50,
                    passed: 50,
                    failed: 0,
                    ignored: 0,
                    elapsed_secs: 3.0,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "test_failed",
            label: "TEST FAILED",
            grammar: "TEST FAILED total=<N> passed=<N> failed=<N> ignored=<N> [failures=[<name1>, <name2>, ...]] in <SECS>s [<runner>]",
            notes: "failures list capped at 8 names. Names match the runner's identifier scheme.",
            fields: vec![
                FieldDoc {
                    name: "total",
                    ty: "u64",
                    optional: false,
                    description: "passed + failed + ignored.",
                },
                FieldDoc {
                    name: "failures",
                    ty: "list<string>",
                    optional: true,
                    description: "Up to 8 test identifiers; absent when no names were captured.",
                },
                FieldDoc {
                    name: "runner",
                    ty: "enum",
                    optional: false,
                    description: "`nextest` or `legacy`.",
                },
            ],
            examples: vec![
                Summary::TestFailed {
                    runner: TestRunner::Nextest,
                    total: 12,
                    passed: 11,
                    failed: 1,
                    ignored: 0,
                    failures: vec!["my_crate::tests::my_test".into()],
                    elapsed_secs: 12.0,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "test_build_failed",
            label: "TEST BUILD-FAILED",
            grammar: "TEST BUILD-FAILED in <SECS>s [<diag1> | <diag2> | ...] [<runner>]",
            notes: "Emitted when cargo failed to compile before any test ran. Diagnostics capped at 6.",
            fields: vec![
                FieldDoc {
                    name: "diagnostics",
                    ty: "list<string>",
                    optional: false,
                    description: "Up to 6 cargo stderr lines starting with `error[` or `error: `.",
                },
                FieldDoc {
                    name: "runner",
                    ty: "enum",
                    optional: false,
                    description: "`nextest` or `legacy`.",
                },
            ],
            examples: vec![
                Summary::TestBuildFailed {
                    runner: TestRunner::Nextest,
                    errors: vec!["error[E0599]: no method named `foo`".into()],
                    elapsed_secs: 6.2,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "clippy_ok",
            label: "CLIPPY OK",
            grammar: "CLIPPY OK warnings=<N> in <SECS>s",
            notes: "warnings is always emitted, even when zero, to distinguish from BUILD OK.",
            fields: vec![FieldDoc {
                name: "warnings",
                ty: "u64",
                optional: false,
                description: "Count of `warning:` lines on stderr.",
            }],
            examples: vec![
                Summary::ClippyOk {
                    warnings: 0,
                    elapsed_secs: 4.2,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "clippy_failed",
            label: "CLIPPY FAILED",
            grammar: "CLIPPY FAILED warnings=<N> in <SECS>s [<diag1> | ...]",
            notes: "Diagnostics list contains up to 6 errors (preferred) or warnings if no errors are present.",
            fields: vec![
                FieldDoc {
                    name: "warnings",
                    ty: "u64",
                    optional: false,
                    description: "Total warning count.",
                },
                FieldDoc {
                    name: "diagnostics",
                    ty: "list<string>",
                    optional: false,
                    description: "Up to 6 stderr lines.",
                },
            ],
            examples: vec![
                Summary::ClippyFailed {
                    warnings: 4,
                    diagnostics: vec!["warning: unused import: `std::io`".into()],
                    elapsed_secs: 4.1,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "timeout",
            label: "TIMEOUT",
            grammar: "TIMEOUT after <SECS>s (limit <LIMIT>s) -- child killed",
            notes: "Emitted instead of any other summary when --timeout fires. Process exits 124.",
            fields: vec![
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Elapsed wall-clock seconds at the moment the child was killed.",
                },
                FieldDoc {
                    name: "limit_secs",
                    ty: "u64",
                    optional: false,
                    description: "The --timeout value.",
                },
            ],
            examples: vec![
                Summary::Timeout {
                    elapsed_secs: 600.0,
                    limit_secs: 600,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "heartbeat",
            label: "HEARTBEAT (stderr, not a summary)",
            grammar: "[heartbeat <SECS>s stdout=<N> stderr=<N> +<N> since-last [latest=\"<excerpt>\"]]",
            notes: "Emitted on stderr while the child runs. Not a summary line. The excerpt is truncated to 120 chars with an '...' suffix.",
            fields: vec![
                FieldDoc {
                    name: "secs",
                    ty: "f64 (zero decimals)",
                    optional: false,
                    description: "Elapsed wall-clock seconds since spawn.",
                },
                FieldDoc {
                    name: "stdout",
                    ty: "u64",
                    optional: false,
                    description: "Cumulative stdout line count.",
                },
                FieldDoc {
                    name: "stderr",
                    ty: "u64",
                    optional: false,
                    description: "Cumulative stderr line count.",
                },
                FieldDoc {
                    name: "since-last",
                    ty: "u64",
                    optional: false,
                    description: "Lines seen since the previous heartbeat reset.",
                },
                FieldDoc {
                    name: "latest",
                    ty: "string",
                    optional: true,
                    description: "Most recent non-blank line, quoted, truncated to 120 chars.",
                },
            ],
            examples: vec![
                r#"[heartbeat 30s stdout=12 stderr=4 +6 since-last latest="Compiling foo v0.1.0"]"#
                    .into(),
            ],
        },
    ]
}

/// Render the human-readable form of the output documentation (the
/// content of `cargo summary --describe-output`).
pub fn output_doc_text(wrapper_version: &str) -> String {
    let mut out = String::new();
    out.push_str("cargo-summary output description (doc version ");
    out.push_str(OUTPUT_DOC_VERSION);
    out.push_str(", wrapper ");
    out.push_str(wrapper_version);
    out.push_str(")\n\n");

    out.push_str("Summarizable subcommands\n");
    out.push_str("  All other cargo subcommands are forwarded raw (see --wrap-unknown).\n\n");
    for sub in subcommand_docs() {
        out.push_str("  cargo ");
        out.push_str(sub.name);
        out.push('\n');
        out.push_str("    Invocation:    ");
        out.push_str(sub.cargo_invocation);
        out.push('\n');
        out.push_str("    Summary kinds: ");
        out.push_str(&sub.summary_kinds.join(", "));
        out.push('\n');
        if !sub.notes.is_empty() {
            out.push_str("    Notes:         ");
            out.push_str(sub.notes);
            out.push('\n');
        }
        out.push('\n');
    }

    let cap = log_capture_doc();
    out.push_str("Log capture\n");
    out.push_str("  ");
    out.push_str(cap.description);
    out.push_str("\n  Suffix grammar:");
    out.push_str(cap.suffix_grammar);
    out.push_str("\n  File layout:   ");
    out.push_str(cap.file_layout);
    out.push_str("\n\n");

    for schema in output_doc_lines() {
        out.push_str(schema.label);
        out.push('\n');
        out.push_str("    Grammar: ");
        out.push_str(schema.grammar);
        out.push('\n');
        if !schema.notes.is_empty() {
            out.push_str("    Notes:   ");
            out.push_str(schema.notes);
            out.push('\n');
        }
        if !schema.fields.is_empty() {
            out.push_str("    Fields:\n");
            for f in &schema.fields {
                let opt = if f.optional { " (optional)" } else { "" };
                let desc = if f.description.is_empty() {
                    String::new()
                } else {
                    format!(" - {}", f.description)
                };
                out.push_str(&format!("        {}: {}{}{}\n", f.name, f.ty, opt, desc));
            }
        }
        out.push_str("    Examples:\n");
        for ex in &schema.examples {
            out.push_str("        ");
            out.push_str(ex);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// Build the full output documentation document (the content of
/// `cargo summary --describe-output-json` after pretty-printing).
pub fn output_doc(wrapper_version: &'static str) -> OutputDoc {
    OutputDoc {
        schema: OUTPUT_DOC_SCHEMA_URN,
        doc_version: OUTPUT_DOC_VERSION,
        wrapper_version,
        subcommands: subcommand_docs(),
        log_capture: log_capture_doc(),
        lines: output_doc_lines(),
    }
}

/// Render the output documentation as pretty-printed JSON.
pub fn output_doc_json(wrapper_version: &'static str) -> String {
    serde_json::to_string_pretty(&output_doc(wrapper_version)).expect("output doc is serializable")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn s(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    // ---- Subcommand classification ----

    #[test]
    fn classifies_subcommands() {
        for n in SubcommandKind::ALL_NAMES {
            assert!(
                SubcommandKind::for_name(n).is_some(),
                "name {} not classifiable",
                n
            );
        }
        for n in ["run", "doc", "fmt", "bench", "", "BUILD", " test"] {
            assert_eq!(
                SubcommandKind::for_name(n),
                None,
                "name {:?} was unexpectedly classifiable",
                n
            );
        }
    }

    #[test]
    fn mode_labels_are_uppercase() {
        for n in SubcommandKind::ALL_NAMES {
            let k = SubcommandKind::for_name(n).unwrap();
            assert_eq!(k.mode_label(), n.to_uppercase().as_str());
        }
    }

    // ---- CargoVersion parsing ----

    #[test]
    fn parses_cargo_version_string() {
        let v = CargoVersion::parse("cargo 1.94.1 (29ea6fb6a 2026-03-24)").unwrap();
        assert_eq!(
            v,
            CargoVersion {
                major: 1,
                minor: 94,
                patch: 1
            }
        );
    }

    #[test]
    fn parses_cargo_nightly_version() {
        let v = CargoVersion::parse("cargo 1.95.0-nightly (abc 2026-04-01)").unwrap();
        assert_eq!(
            v,
            CargoVersion {
                major: 1,
                minor: 95,
                patch: 0
            }
        );
    }

    #[test]
    fn parses_two_component_version() {
        // Older toolchains sometimes printed without patch.
        let v = CargoVersion::parse("cargo 1.90 (deadbeef)").unwrap();
        assert_eq!(
            v,
            CargoVersion {
                major: 1,
                minor: 90,
                patch: 0
            }
        );
    }

    #[test]
    fn rejects_garbage_version() {
        assert!(CargoVersion::parse("cargo whatever").is_none());
        assert!(CargoVersion::parse("").is_none());
        assert!(CargoVersion::parse("rustc 1.94.1").is_some()); // permissive on the program name
        assert!(CargoVersion::parse("cargo").is_none());
        assert!(CargoVersion::parse("cargo 1").is_none());
    }

    #[test]
    fn cargo_versions_order_by_components() {
        let a = CargoVersion {
            major: 1,
            minor: 89,
            patch: 0,
        };
        let b = CargoVersion {
            major: 1,
            minor: 90,
            patch: 0,
        };
        let c = CargoVersion {
            major: 1,
            minor: 90,
            patch: 1,
        };
        let d = CargoVersion {
            major: 2,
            minor: 0,
            patch: 0,
        };
        assert!(a < b && b < c && c < d);
    }

    // ---- Summary rendering ----

    #[test]
    fn renders_build_ok_without_warnings() {
        let s = Summary::BuildOk {
            mode: SubcommandKind::Build,
            warnings: 0,
            elapsed_secs: 12.34,
        }
        .render();
        assert_eq!(s, "BUILD OK in 12.3s");
    }

    #[test]
    fn renders_build_ok_with_warnings() {
        let s = Summary::BuildOk {
            mode: SubcommandKind::Check,
            warnings: 2,
            elapsed_secs: 4.1,
        }
        .render();
        assert_eq!(s, "CHECK OK warnings=2 in 4.1s");
    }

    #[test]
    fn renders_build_failed() {
        let s = Summary::BuildFailed {
            mode: SubcommandKind::Build,
            errors: vec!["error[E0599]: foo".into(), "error: bar".into()],
            elapsed_secs: 6.2,
        }
        .render();
        assert_eq!(s, "BUILD FAILED in 6.2s [error[E0599]: foo | error: bar]");
    }

    #[test]
    fn renders_build_failed_empty() {
        let s = Summary::BuildFailed {
            mode: SubcommandKind::Build,
            errors: vec![],
            elapsed_secs: 1.0,
        }
        .render();
        assert_eq!(s, "BUILD FAILED in 1.0s [no error lines captured]");
    }

    #[test]
    fn renders_test_ok_nextest() {
        let s = Summary::TestOk {
            runner: TestRunner::Nextest,
            total: 10,
            passed: 9,
            failed: 0,
            ignored: 1,
            elapsed_secs: 5.0,
        }
        .render();
        assert_eq!(
            s,
            "TEST OK total=10 passed=9 failed=0 ignored=1 in 5.0s [nextest]"
        );
    }

    #[test]
    fn renders_test_failed_with_failures() {
        let s = Summary::TestFailed {
            runner: TestRunner::Legacy,
            total: 12,
            passed: 11,
            failed: 1,
            ignored: 0,
            failures: vec!["mod::my_test".into()],
            elapsed_secs: 12.0,
        }
        .render();
        assert_eq!(
            s,
            "TEST FAILED total=12 passed=11 failed=1 ignored=0 failures=[mod::my_test] in 12.0s [legacy]"
        );
    }

    #[test]
    fn renders_test_failed_without_failure_names() {
        let s = Summary::TestFailed {
            runner: TestRunner::Nextest,
            total: 5,
            passed: 4,
            failed: 1,
            ignored: 0,
            failures: vec![],
            elapsed_secs: 1.0,
        }
        .render();
        assert!(
            !s.contains("failures="),
            "no failure names should not produce a failures= field: {}",
            s
        );
    }

    #[test]
    fn renders_test_build_failed_with_runner_tag() {
        let s = Summary::TestBuildFailed {
            runner: TestRunner::Nextest,
            errors: vec!["error[E0599]: x".into()],
            elapsed_secs: 6.2,
        }
        .render();
        assert_eq!(s, "TEST BUILD-FAILED in 6.2s [error[E0599]: x] [nextest]");
    }

    #[test]
    fn renders_test_build_failed_empty() {
        let s = Summary::TestBuildFailed {
            runner: TestRunner::Legacy,
            errors: vec![],
            elapsed_secs: 0.5,
        }
        .render();
        assert_eq!(
            s,
            "TEST BUILD-FAILED in 0.5s [no error lines captured] [legacy]"
        );
    }

    #[test]
    fn renders_clippy_ok_always_shows_zero() {
        let s = Summary::ClippyOk {
            warnings: 0,
            elapsed_secs: 4.2,
        }
        .render();
        assert_eq!(s, "CLIPPY OK warnings=0 in 4.2s");
    }

    #[test]
    fn renders_clippy_failed() {
        let s = Summary::ClippyFailed {
            warnings: 4,
            diagnostics: vec!["warning: unused import".into()],
            elapsed_secs: 4.1,
        }
        .render();
        assert_eq!(
            s,
            "CLIPPY FAILED warnings=4 in 4.1s [warning: unused import]"
        );
    }

    #[test]
    fn renders_clippy_failed_empty_diagnostics() {
        let s = Summary::ClippyFailed {
            warnings: 0,
            diagnostics: vec![],
            elapsed_secs: 1.0,
        }
        .render();
        assert_eq!(
            s,
            "CLIPPY FAILED warnings=0 in 1.0s [no diagnostic lines captured]"
        );
    }

    #[test]
    fn renders_timeout_uses_ascii_dashes() {
        let s = Summary::Timeout {
            elapsed_secs: 600.0,
            limit_secs: 600,
        }
        .render();
        assert_eq!(s, "TIMEOUT after 600.0s (limit 600s) -- child killed");
        assert!(!s.contains('\u{2014}'));
    }

    // ---- Summarizer behavior ----

    #[test]
    fn summarize_build_counts_warnings_on_success() {
        let stderr = s(&[
            "   Compiling foo v0.1.0",
            "warning: unused import: `std::io`",
            "warning: dead code",
            "    Finished `dev` profile",
        ]);
        let out = summarize_build(&stderr, true, 2.5, SubcommandKind::Build);
        assert_eq!(out.render(), "BUILD OK warnings=2 in 2.5s");
    }

    #[test]
    fn summarize_build_captures_errors_on_failure() {
        let stderr = s(&[
            "   Compiling foo v0.1.0",
            "error[E0599]: no method named `foo`",
            "    --> src/lib.rs:1:1",
        ]);
        let out = summarize_build(&stderr, false, 1.0, SubcommandKind::Check);
        assert_eq!(
            out.render(),
            "CHECK FAILED in 1.0s [error[E0599]: no method named `foo`]"
        );
    }

    #[test]
    fn summarize_build_caps_error_lines() {
        let stderr: Vec<String> = (0..50).map(|i| format!("error: e{}", i)).collect();
        let out = summarize_build(&stderr, false, 1.0, SubcommandKind::Build);
        match out {
            Summary::BuildFailed { errors, .. } => {
                assert_eq!(errors.len(), MAX_DIAGNOSTIC_LINES);
                assert_eq!(errors[0], "error: e0");
                assert_eq!(
                    errors[MAX_DIAGNOSTIC_LINES - 1],
                    format!("error: e{}", MAX_DIAGNOSTIC_LINES - 1)
                );
            }
            other => panic!("expected BuildFailed, got {:?}", other),
        }
    }

    #[test]
    fn summarize_build_failed_with_no_error_lines() {
        let stderr = s(&["compiler internal error or oom or something"]);
        let out = summarize_build(&stderr, false, 0.1, SubcommandKind::Build);
        assert_eq!(
            out.render(),
            "BUILD FAILED in 0.1s [no error lines captured]"
        );
    }

    #[test]
    fn summarize_test_legacy_parses_test_result() {
        let stdout =
            s(&["test result: ok. 10 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out"]);
        let out = summarize_test_legacy(&stdout, &[], true, 3.0);
        assert_eq!(
            out.render(),
            "TEST OK total=11 passed=10 failed=0 ignored=1 in 3.0s [legacy]"
        );
    }

    #[test]
    fn summarize_test_legacy_captures_failed_names() {
        let stdout = s(&[
            "test my_test ... FAILED",
            "test other_test ... ok",
            "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured",
        ]);
        let out = summarize_test_legacy(&stdout, &[], false, 1.0);
        assert_eq!(
            out.render(),
            "TEST FAILED total=2 passed=1 failed=1 ignored=0 failures=[my_test] in 1.0s [legacy]"
        );
    }

    #[test]
    fn summarize_test_legacy_caps_failure_names_at_eight() {
        let mut lines: Vec<String> = (0..20).map(|i| format!("test t{} ... FAILED", i)).collect();
        lines.push("test result: FAILED. 0 passed; 20 failed; 0 ignored; 0 measured".to_string());
        let out = summarize_test_legacy(&lines, &[], false, 1.0);
        match out {
            Summary::TestFailed {
                failures, failed, ..
            } => {
                assert_eq!(failed, 20);
                assert_eq!(failures.len(), MAX_FAILURE_NAMES);
            }
            other => panic!("expected TestFailed, got {:?}", other),
        }
    }

    #[test]
    fn summarize_test_legacy_classifies_compile_errors() {
        let out = summarize_test_legacy(&[], &s(&["error[E0432]: unresolved import"]), false, 0.5);
        assert_eq!(
            out.render(),
            "TEST BUILD-FAILED in 0.5s [error[E0432]: unresolved import] [legacy]"
        );
    }

    #[test]
    fn summarize_test_legacy_empty_input_failing() {
        // No test result line, ok=false: treat as TEST FAILED with all zeros.
        let out = summarize_test_legacy(&[], &[], false, 0.5);
        assert!(
            out.render().starts_with("TEST FAILED total=0"),
            "unexpected: {}",
            out.render()
        );
    }

    #[test]
    fn summarize_test_legacy_unknown_test_result_format_is_lenient() {
        // Garbage in the test-result line tokens shouldn't panic; counts stay zero.
        let stdout = s(&["test result: ok. wat passed; ohno failed; 1 ignored"]);
        let out = summarize_test_legacy(&stdout, &[], true, 1.0);
        match out {
            Summary::TestOk {
                passed,
                failed,
                ignored,
                ..
            } => {
                assert_eq!((passed, failed, ignored), (0, 0, 1));
            }
            other => panic!("expected TestOk, got {:?}", other),
        }
    }

    #[test]
    fn summarize_test_nextest_parses_suite_event() {
        let stdout = s(&[r#"{"type":"suite","event":"ok","passed":42,"failed":0,"ignored":3}"#]);
        let out = summarize_test_nextest(&stdout, &[], true, 1.0);
        assert_eq!(
            out.render(),
            "TEST OK total=45 passed=42 failed=0 ignored=3 in 1.0s [nextest]"
        );
    }

    #[test]
    fn summarize_test_nextest_aggregates_multiple_suite_events() {
        let stdout = s(&[
            r#"{"type":"suite","event":"ok","passed":10,"failed":0,"ignored":1}"#,
            r#"{"type":"suite","event":"ok","passed":5,"failed":0,"ignored":0}"#,
        ]);
        let out = summarize_test_nextest(&stdout, &[], true, 1.0);
        assert_eq!(
            out.render(),
            "TEST OK total=16 passed=15 failed=0 ignored=1 in 1.0s [nextest]"
        );
    }

    #[test]
    fn summarize_test_nextest_skips_malformed_json_lines() {
        let stdout = s(&[
            "not json at all",
            r#"{this is invalid json}"#,
            r#"{"type":"suite","event":"ok","passed":1,"failed":0,"ignored":0}"#,
            "",
        ]);
        let out = summarize_test_nextest(&stdout, &[], true, 0.1);
        assert_eq!(
            out.render(),
            "TEST OK total=1 passed=1 failed=0 ignored=0 in 0.1s [nextest]"
        );
    }

    #[test]
    fn summarize_test_nextest_captures_test_failures() {
        let stdout = s(&[
            r#"{"type":"test","event":"failed","name":"crate::tests::my_test"}"#,
            r#"{"type":"suite","event":"failed","passed":0,"failed":1,"ignored":0}"#,
        ]);
        let out = summarize_test_nextest(&stdout, &[], false, 1.0);
        let rendered = out.render();
        assert!(
            rendered.contains("failures=[crate::tests::my_test]"),
            "got: {}",
            rendered
        );
        assert!(rendered.contains("[nextest]"));
    }

    #[test]
    fn summarize_test_nextest_compile_error_only_when_no_suite() {
        let stderr = s(&["error[E0001]: x"]);
        // Compile error and no suite event -> TEST BUILD-FAILED.
        let out = summarize_test_nextest(&[], &stderr, false, 0.5);
        assert!(out.render().starts_with("TEST BUILD-FAILED"));

        // Compile error AFTER a suite event -> regular TEST FAILED.
        let stdout = s(&[r#"{"type":"suite","event":"failed","passed":1,"failed":1,"ignored":0}"#]);
        let out = summarize_test_nextest(&stdout, &stderr, false, 0.5);
        assert!(out.render().starts_with("TEST FAILED total=2"));
    }

    #[test]
    fn summarize_test_nextest_strips_binary_dollar_prefix() {
        let stdout = s(&[
            r#"{"type":"test","event":"failed","name":"my_binary$crate::tests::my_test"}"#,
            r#"{"type":"suite","event":"failed","passed":0,"failed":1,"ignored":0}"#,
        ]);
        let out = summarize_test_nextest(&stdout, &[], false, 1.0);
        assert!(out.render().contains("failures=[crate::tests::my_test]"));
    }

    #[test]
    fn summarize_clippy_zero_warnings_on_success() {
        let out = summarize_clippy(&[], true, 1.0);
        assert_eq!(out.render(), "CLIPPY OK warnings=0 in 1.0s");
    }

    #[test]
    fn summarize_clippy_uses_warnings_when_no_errors() {
        let out = summarize_clippy(&s(&["warning: unused"]), false, 1.0);
        assert_eq!(
            out.render(),
            "CLIPPY FAILED warnings=1 in 1.0s [warning: unused]"
        );
    }

    #[test]
    fn summarize_clippy_prefers_errors_over_warnings() {
        let stderr = s(&[
            "warning: unused",
            "error[E0599]: bad call",
            "warning: another",
        ]);
        let out = summarize_clippy(&stderr, false, 1.0);
        assert_eq!(
            out.render(),
            "CLIPPY FAILED warnings=2 in 1.0s [error[E0599]: bad call]"
        );
    }

    // ---- Path relativization ----

    #[test]
    fn relativize_against_returns_subpath_when_target_is_under_base() {
        assert_eq!(
            relativize_against(
                Path::new("/home/u/proj/target/x/y.log"),
                Path::new("/home/u/proj")
            ),
            PathBuf::from("target/x/y.log"),
        );
    }

    #[test]
    fn relativize_against_returns_dot_when_equal() {
        let p = Path::new("/home/u/proj");
        assert_eq!(relativize_against(p, p), PathBuf::from("."));
    }

    #[test]
    fn relativize_against_uses_parent_dots_when_outside_base() {
        let base = Path::new("/home/u/proj/src/bin");
        let target = Path::new("/home/u/proj/target/cargo_summary/build.log");
        assert_eq!(
            relativize_against(target, base),
            PathBuf::from("../../target/cargo_summary/build.log"),
        );
    }

    #[test]
    fn relativize_against_does_not_panic_on_unicode() {
        let base = Path::new("/home/u/проект");
        let target = Path::new("/home/u/проект/target/файл.log");
        assert_eq!(
            relativize_against(target, base),
            PathBuf::from("target/файл.log"),
        );
    }

    #[test]
    fn relativize_against_handles_relative_inputs() {
        let base = Path::new("a/b");
        let target = Path::new("a/b/c");
        assert_eq!(relativize_against(target, base), PathBuf::from("c"));
    }

    // ---- Output documentation invariants ----

    #[test]
    fn output_doc_schema_is_valid_json() {
        let v: serde_json::Value =
            serde_json::from_str(OUTPUT_DOC_SCHEMA).expect("OUTPUT_DOC_SCHEMA must be valid JSON");
        assert_eq!(
            v.get("$schema").and_then(|s| s.as_str()),
            Some("https://json-schema.org/draft/2020-12/schema"),
        );
        assert_eq!(
            v.get("$id").and_then(|s| s.as_str()),
            Some(OUTPUT_DOC_SCHEMA_URN),
        );
    }

    #[test]
    fn output_doc_json_carries_schema_urn_and_subcommands() {
        let raw = output_doc_json("0.0.0-test");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            v.get("schema").and_then(|s| s.as_str()),
            Some(OUTPUT_DOC_SCHEMA_URN),
        );
        assert_eq!(
            v.get("wrapper_version").and_then(|s| s.as_str()),
            Some("0.0.0-test")
        );
        let subs = v
            .get("subcommands")
            .and_then(|s| s.as_array())
            .expect("subcommands");
        let names: Vec<&str> = subs
            .iter()
            .map(|s| s.get("name").and_then(|n| n.as_str()).unwrap_or(""))
            .collect();
        for n in SubcommandKind::ALL_NAMES {
            assert!(names.contains(n), "subcommand {} missing from doc", n);
        }
    }

    #[test]
    fn output_doc_text_mentions_every_subcommand() {
        let text = output_doc_text("0.0.0-test");
        for n in SubcommandKind::ALL_NAMES {
            assert!(
                text.contains(&format!("cargo {}", n)),
                "missing 'cargo {}' in text doc",
                n
            );
        }
    }

    #[test]
    fn output_doc_text_lists_every_summary_kind() {
        let text = output_doc_text("0.0.0-test");
        for label in [
            "BUILD OK / CHECK OK",
            "BUILD FAILED / CHECK FAILED",
            "TEST OK",
            "TEST FAILED",
            "TEST BUILD-FAILED",
            "CLIPPY OK",
            "CLIPPY FAILED",
            "TIMEOUT",
            "HEARTBEAT",
        ] {
            assert!(text.contains(label), "output doc missing '{}'", label);
        }
    }

    #[test]
    fn output_doc_lines_kinds_are_unique() {
        let kinds: Vec<&str> = output_doc_lines().iter().map(|l| l.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), kinds.len(), "duplicate kinds: {:?}", kinds);
    }

    #[test]
    fn output_doc_subcommands_reference_valid_line_kinds() {
        let line_kinds: std::collections::HashSet<&str> =
            output_doc_lines().iter().map(|l| l.kind).collect();
        for sub in subcommand_docs() {
            for k in sub.summary_kinds {
                assert!(
                    line_kinds.contains(k),
                    "subcommand '{}' references unknown kind '{}'",
                    sub.name,
                    k
                );
            }
        }
    }
}
