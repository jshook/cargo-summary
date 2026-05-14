// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Parsers that build [`crate::Summary`] values from captured cargo /
//! nextest output. Each cargo subcommand cargo-summary recognizes has
//! its own submodule; common caps live here.

pub mod build;
pub mod clippy;
pub mod test_legacy;
pub mod test_nextest;

pub use build::summarize_build;
pub use clippy::summarize_clippy;
pub use test_legacy::summarize_test_legacy;
pub use test_nextest::summarize_test_nextest;

/// Cap on the number of failure names captured in
/// [`crate::Summary::TestFailed`].
pub const MAX_FAILURE_NAMES: usize = 8;

/// Cap on the number of diagnostic lines captured in
/// [`crate::Summary::BuildFailed`], [`crate::Summary::TestBuildFailed`],
/// and [`crate::Summary::ClippyFailed`].
pub const MAX_DIAGNOSTIC_LINES: usize = 6;
