// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Path relativization used to render readable log paths in summary lines.
//!
//! The CLI calls [`relativize_to_cwd`]; [`relativize_against`] is the
//! generic primitive (and the part with the interesting edge cases).

use std::path::{Component, Path, PathBuf};

/// Make `path` relative to the current working directory, if possible.
///
/// Falls back to the input unchanged when the CWD is not available or
/// the two paths cannot share a root (e.g. different Windows drives).
#[must_use]
pub fn relativize_to_cwd(path: &Path) -> PathBuf {
    let Ok(cwd) = std::env::current_dir() else {
        return path.to_path_buf();
    };
    relativize_against(path, &cwd)
}

/// Compute `path` relative to `base`.
///
/// When the target lives under the base, the result is a subpath;
/// otherwise the result uses `..` to ascend out of `base` to a shared
/// ancestor. Paths with no shared root anchor are returned unchanged.
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
#[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
