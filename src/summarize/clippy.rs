// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Parser for `cargo clippy` stderr. Like the build parser plus
//! mandatory warning count, since `CLIPPY OK warnings=N` is how the
//! line distinguishes itself from `BUILD OK`.

use super::MAX_DIAGNOSTIC_LINES;
use crate::Summary;

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
#[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s(strs: &[&str]) -> Vec<String> {
        strs.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn zero_warnings_on_success() {
        let out = summarize_clippy(&[], true, 1.0);
        assert_eq!(out.render(), "CLIPPY OK warnings=0 in 1.0s");
    }

    #[test]
    fn uses_warnings_when_no_errors() {
        let out = summarize_clippy(&s(&["warning: unused"]), false, 1.0);
        assert_eq!(
            out.render(),
            "CLIPPY FAILED warnings=1 in 1.0s [warning: unused]"
        );
    }

    #[test]
    fn prefers_errors_over_warnings() {
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
}
