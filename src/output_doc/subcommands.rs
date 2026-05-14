// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Per-subcommand documentation: the exact cargo invocation
//! cargo-summary runs for each summarizable subcommand and the
//! summary `kind`s it can produce.

use super::SubcommandDoc;

/// Per-subcommand documentation, including the exact cargo
/// invocation cargo-summary runs and which summary kinds it can
/// produce.
#[must_use]
pub fn subcommand_docs() -> Vec<SubcommandDoc> {
    vec![
        SubcommandDoc {
            name: "build",
            cargo_invocation: "cargo build [args...]",
            summary_kinds: &["build_ok", "build_failed", "timeout"],
            notes: "Warning count comes from stderr lines starting with `warning:`. Error capture takes up to 6 stderr lines starting with `error[` or `error: `.",
        },
        SubcommandDoc {
            name: "check",
            cargo_invocation: "cargo check [args...]",
            summary_kinds: &["build_ok", "build_failed", "timeout"],
            notes: "Identical parsing to `build`; emits the CHECK mode label instead of BUILD.",
        },
        SubcommandDoc {
            name: "test",
            cargo_invocation: "cargo nextest run --message-format libtest-json --message-format-version 0.1 --no-fail-fast [args...] | cargo test [args...]",
            summary_kinds: &["test_ok", "test_failed", "test_build_failed", "timeout"],
            notes: "When `cargo-nextest` is installed and `--no-nextest` is not set, the nextest path is used: counts come from `suite` events with `event=ok|failed`, and failure names come from `test` events with `event=failed`. Otherwise the legacy path parses libtest's `test result: ...` summary line and `test NAME ... FAILED` lines. Failure name list capped at 8. Either path yields a TEST BUILD-FAILED summary when cargo fails to compile before any test runs.",
        },
        SubcommandDoc {
            name: "clippy",
            cargo_invocation: "cargo clippy [args...]",
            summary_kinds: &["clippy_ok", "clippy_failed", "timeout"],
            notes: "Warning count is always emitted (even at zero) to distinguish from BUILD OK. On failure, the diagnostics list prefers up to 6 `error[`/`error: ` lines, falling back to `warning:` lines if no errors were captured.",
        },
    ]
}
