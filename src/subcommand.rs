// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Classification of cargo subcommands cargo-summary explicitly
//! parses (build / check / test / clippy).

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
    #[must_use]
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
    #[must_use]
    pub const fn mode_label(&self) -> &'static str {
        match self {
            Self::Build => "BUILD",
            Self::Check => "CHECK",
            Self::Test => "TEST",
            Self::Clippy => "CLIPPY",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_subcommands() {
        for n in SubcommandKind::ALL_NAMES {
            assert!(
                SubcommandKind::for_name(n).is_some(),
                "name {n} not classifiable"
            );
        }
        for n in ["run", "doc", "fmt", "bench", "", "BUILD", " test"] {
            assert_eq!(
                SubcommandKind::for_name(n),
                None,
                "name {n:?} was unexpectedly classifiable"
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
}
