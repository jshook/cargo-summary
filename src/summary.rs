// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Canonical representation of the single line cargo-summary emits per invocation.
//!
//! The [`Summary`] enum has one variant per line shape;
//! [`Summary::render`] is the only place wire-format strings are
//! constructed, so every documented example in the output
//! documentation can round-trip through the same code path.

use crate::SubcommandKind;

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
    #[must_use]
    pub const fn tag(&self) -> &'static str {
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
    // A single match over the Summary variants; each arm is the
    // canonical wire form for one line shape. Splitting into helpers
    // would scatter the format across the file without improving
    // legibility.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::BuildOk {
                mode,
                warnings,
                elapsed_secs,
            } => {
                let warns = if *warnings > 0 {
                    format!(" warnings={warnings}")
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
            } => format!("CLIPPY OK warnings={warnings} in {elapsed_secs:.1}s"),
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
                format!("CLIPPY FAILED warnings={warnings} in {elapsed_secs:.1}s [{body}]")
            }
            Self::Timeout {
                elapsed_secs,
                limit_secs,
            } => format!("TIMEOUT after {elapsed_secs:.1}s (limit {limit_secs}s) -- child killed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "no failure names should not produce a failures= field: {s}"
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
}
