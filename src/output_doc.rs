// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Self-describing output documentation: the data behind
//! `cargo summary --describe-output[-json]` and its JSON Schema.
//!
//! The runtime [`output_doc`] constructs the in-memory [`OutputDoc`]
//! tree, [`text::output_doc_text`] renders the human form, and
//! [`json::output_doc_json`] renders the JSON form. Every documented
//! example is produced by the same [`crate::Summary::render`] the
//! live tool uses, so documentation cannot drift from emitted output.

pub mod json;
pub mod lines;
pub mod schema;
pub mod subcommands;
pub mod text;

pub use json::output_doc_json;
pub use lines::output_doc_lines;
pub use schema::{OUTPUT_DOC_SCHEMA, OUTPUT_DOC_SCHEMA_URN, OUTPUT_DOC_VERSION};
pub use subcommands::subcommand_docs;
pub use text::output_doc_text;

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

/// Log-capture metadata for the output document.
#[must_use]
pub const fn log_capture_doc() -> LogCaptureDoc {
    LogCaptureDoc {
        description: "When the wrapper can create log files, every summary line ends with a logs suffix pointing at the captured stdout/stderr. Omitted on failure to create files. Paths are rendered relative to the process CWD by default; pass --absolute-log-paths for absolute paths.",
        suffix_grammar: " logs.stdout=<cwd-relative-path> logs.stderr=<cwd-relative-path>",
        file_layout: "<workspace-or-package-root>/target/cargo_summary/<mode>-<millis-since-epoch>.{stdout,stderr}.log",
    }
}

/// Build the full output documentation document (the content of
/// `cargo summary --describe-output-json` after pretty-printing).
#[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SubcommandKind;

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
    fn output_doc_lines_kinds_are_unique() {
        let kinds: Vec<&str> = output_doc_lines().iter().map(|l| l.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), kinds.len(), "duplicate kinds: {kinds:?}");
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

    #[test]
    fn output_doc_lists_every_summarizable_subcommand() {
        let names: Vec<&str> = subcommand_docs().iter().map(|s| s.name).collect();
        for n in SubcommandKind::ALL_NAMES {
            assert!(names.contains(n), "subcommand {n} missing from doc");
        }
    }
}
