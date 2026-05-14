// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Parser for `cargo test`'s libtest text output. Counts come from
//! the trailing `test result: ...` line; failure names come from
//! per-test `test NAME ... FAILED` lines.

use super::{MAX_DIAGNOSTIC_LINES, MAX_FAILURE_NAMES};
use crate::{Summary, TestRunner};

/// Build a `Summary` from a captured `cargo test` (libtest) run.
///
/// Parses `test result: ...` lines for counts and `test NAME ... FAILED`
/// lines for failure names. See also [`crate::summarize_test_nextest`]
/// for the JSON path.
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
#[must_use]
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
                if let Some(rest) = token.strip_suffix(" passed")
                    && let Some(num) = rest.split_whitespace().next_back()
                {
                    passed += num.parse::<u64>().unwrap_or(0);
                } else if let Some(rest) = token.strip_suffix(" failed")
                    && let Some(num) = rest.split_whitespace().next_back()
                {
                    failed += num.parse::<u64>().unwrap_or(0);
                } else if let Some(rest) = token.strip_suffix(" ignored")
                    && let Some(num) = rest.split_whitespace().next_back()
                {
                    ignored += num.parse::<u64>().unwrap_or(0);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s(strs: &[&str]) -> Vec<String> {
        strs.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn parses_test_result() {
        let stdout =
            s(&["test result: ok. 10 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out"]);
        let out = summarize_test_legacy(&stdout, &[], true, 3.0);
        assert_eq!(
            out.render(),
            "TEST OK total=11 passed=10 failed=0 ignored=1 in 3.0s [legacy]"
        );
    }

    #[test]
    fn captures_failed_names() {
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
    fn caps_failure_names_at_eight() {
        let mut lines: Vec<String> = (0..20).map(|i| format!("test t{i} ... FAILED")).collect();
        lines.push("test result: FAILED. 0 passed; 20 failed; 0 ignored; 0 measured".to_string());
        let out = summarize_test_legacy(&lines, &[], false, 1.0);
        match out {
            Summary::TestFailed {
                failures, failed, ..
            } => {
                assert_eq!(failed, 20);
                assert_eq!(failures.len(), MAX_FAILURE_NAMES);
            }
            other => panic!("expected TestFailed, got {other:?}"),
        }
    }

    #[test]
    fn classifies_compile_errors() {
        let out = summarize_test_legacy(&[], &s(&["error[E0432]: unresolved import"]), false, 0.5);
        assert_eq!(
            out.render(),
            "TEST BUILD-FAILED in 0.5s [error[E0432]: unresolved import] [legacy]"
        );
    }

    #[test]
    fn empty_input_failing() {
        // No test result line, ok=false: treat as TEST FAILED with all zeros.
        let out = summarize_test_legacy(&[], &[], false, 0.5);
        assert!(
            out.render().starts_with("TEST FAILED total=0"),
            "unexpected: {}",
            out.render()
        );
    }

    #[test]
    fn unknown_test_result_format_is_lenient() {
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
            other => panic!("expected TestOk, got {other:?}"),
        }
    }
}
