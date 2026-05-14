// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Parser for the line-delimited libtest-json event stream emitted by nextest.
//!
//! Counts come from `suite` events; failure names come from per-test
//! `failed` events. Cargo's stderr is consulted as a fallback for
//! the compile-error-before-any-test case.

use super::{MAX_DIAGNOSTIC_LINES, MAX_FAILURE_NAMES};
use crate::{Summary, TestRunner};

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
#[must_use]
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
                NextestEvent::Test(t)
                    if t.event == "failed" && failure_names.len() < MAX_FAILURE_NAMES =>
                {
                    let name = t.name.split('$').next_back().unwrap_or(&t.name).to_string();
                    failure_names.push(name);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s(strs: &[&str]) -> Vec<String> {
        strs.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn parses_suite_event() {
        let stdout = s(&[r#"{"type":"suite","event":"ok","passed":42,"failed":0,"ignored":3}"#]);
        let out = summarize_test_nextest(&stdout, &[], true, 1.0);
        assert_eq!(
            out.render(),
            "TEST OK total=45 passed=42 failed=0 ignored=3 in 1.0s [nextest]"
        );
    }

    #[test]
    fn aggregates_multiple_suite_events() {
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
    fn skips_malformed_json_lines() {
        let stdout = s(&[
            "not json at all",
            r"{this is invalid json}",
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
    fn captures_test_failures() {
        let stdout = s(&[
            r#"{"type":"test","event":"failed","name":"crate::tests::my_test"}"#,
            r#"{"type":"suite","event":"failed","passed":0,"failed":1,"ignored":0}"#,
        ]);
        let out = summarize_test_nextest(&stdout, &[], false, 1.0);
        let rendered = out.render();
        assert!(
            rendered.contains("failures=[crate::tests::my_test]"),
            "got: {rendered}"
        );
        assert!(rendered.contains("[nextest]"));
    }

    #[test]
    fn compile_error_only_when_no_suite() {
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
    fn strips_binary_dollar_prefix() {
        let stdout = s(&[
            r#"{"type":"test","event":"failed","name":"my_binary$crate::tests::my_test"}"#,
            r#"{"type":"suite","event":"failed","passed":0,"failed":1,"ignored":0}"#,
        ]);
        let out = summarize_test_nextest(&stdout, &[], false, 1.0);
        assert!(out.render().contains("failures=[crate::tests::my_test]"));
    }
}
