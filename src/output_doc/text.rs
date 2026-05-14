// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Human-readable rendering of the output documentation (the content
//! of `cargo summary --describe-output`).

use super::{OUTPUT_DOC_VERSION, log_capture_doc, output_doc_lines, subcommand_docs};

/// Render the human-readable form of the output documentation (the
/// content of `cargo summary --describe-output`).
#[must_use]
pub fn output_doc_text(wrapper_version: &str) -> String {
    use std::fmt::Write as _;
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
                let _ = writeln!(out, "        {}: {}{}{}", f.name, f.ty, opt, desc);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SubcommandKind;

    #[test]
    fn mentions_every_subcommand() {
        let text = output_doc_text("0.0.0-test");
        for n in SubcommandKind::ALL_NAMES {
            assert!(
                text.contains(&format!("cargo {n}")),
                "missing 'cargo {n}' in text doc"
            );
        }
    }

    #[test]
    fn lists_every_summary_kind() {
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
            assert!(text.contains(label), "output doc missing '{label}'");
        }
    }
}
