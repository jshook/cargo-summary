// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Library half of `cargo-summary`. Exposes the typed summary
//! representation, the parsers that build summaries from cargo /
//! nextest output, the cargo / nextest version detection helpers,
//! the self-describing output documentation, and the path
//! relativization utility used by the CLI.
//!
//! Downstream tools that want to consume cargo-summary lines can
//! depend on the library and build their own pipelines without
//! re-parsing the text form. The CLI binary in `src/bin` is a thin
//! orchestration layer over these primitives.
//!
//! ## Module map
//!
//! - [`subcommand`] — [`SubcommandKind`], the classification of cargo
//!   subcommands cargo-summary recognizes.
//! - [`version`] — cargo and nextest version detection.
//! - [`summary`] — [`Summary`] and [`TestRunner`]; canonical wire form.
//! - [`summarize`] — parsers that build a [`Summary`] from captured
//!   stdout/stderr.
//! - [`path`] — path relativization for the log-suffix in summary lines.
//! - [`output_doc`] — self-describing output documentation and its
//!   JSON Schema.
//!
//! The most commonly-used items are re-exported at the crate root
//! for convenience.
//!
//! ## Versioning
//!
//! Pre-1.0. The public API may change between minor versions; pin
//! exact patch versions in `Cargo.toml` if you depend on it. The
//! emitted text format is documented by [`output_doc_text`] /
//! [`output_doc_json`] and identified by the URN in
//! [`OUTPUT_DOC_SCHEMA_URN`].

pub mod output_doc;
pub mod path;
pub mod subcommand;
pub mod summarize;
pub mod summary;
pub mod version;

pub use output_doc::{
    FieldDoc, LineDoc, LogCaptureDoc, OUTPUT_DOC_SCHEMA, OUTPUT_DOC_SCHEMA_URN, OUTPUT_DOC_VERSION,
    OutputDoc, SubcommandDoc, log_capture_doc, output_doc, output_doc_json, output_doc_lines,
    output_doc_text, subcommand_docs,
};
pub use path::{relativize_against, relativize_to_cwd};
pub use subcommand::SubcommandKind;
pub use summarize::{
    MAX_DIAGNOSTIC_LINES, MAX_FAILURE_NAMES, summarize_build, summarize_clippy,
    summarize_test_legacy, summarize_test_nextest,
};
pub use summary::{Summary, TestRunner};
pub use version::{CargoVersion, cargo_nextest_available, cargo_version, nextest_version};
