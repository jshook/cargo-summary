// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Parser for `cargo build` / `cargo check` stderr. The two cargo
//! subcommands share parsing; only the mode label on the resulting
//! [`crate::Summary`] differs.

use super::MAX_DIAGNOSTIC_LINES;
use crate::{SubcommandKind, Summary};

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
#[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s(strs: &[&str]) -> Vec<String> {
        strs.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn counts_warnings_on_success() {
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
    fn captures_errors_on_failure() {
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
    fn caps_error_lines() {
        let stderr: Vec<String> = (0..50).map(|i| format!("error: e{i}")).collect();
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
            other => panic!("expected BuildFailed, got {other:?}"),
        }
    }

    #[test]
    fn failed_with_no_error_lines() {
        let stderr = s(&["compiler internal error or oom or something"]);
        let out = summarize_build(&stderr, false, 0.1, SubcommandKind::Build);
        assert_eq!(
            out.render(),
            "BUILD FAILED in 0.1s [no error lines captured]"
        );
    }
}
