// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Cargo / nextest version detection.
//!
//! Both detectors are cached at first use via `OnceLock`. Failures
//! (missing binary, unparseable output) yield `None` and are also
//! cached so we don't re-probe on every call.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

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
    #[must_use]
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
///
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
#[must_use]
pub fn cargo_nextest_available() -> bool {
    nextest_version().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
