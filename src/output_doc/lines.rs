// Copyright (c) Jonathan Shook
// SPDX-License-Identifier: Apache-2.0

//! Per-line-shape documentation. The examples in each entry are
//! produced by [`crate::Summary::render`], so the documented format
//! cannot drift from what the live tool emits.

use super::{FieldDoc, LineDoc};
use crate::{SubcommandKind, Summary, TestRunner};

/// Documentation entries for every summary line shape. Each entry's
/// examples are produced by [`Summary::render`], so they cannot
/// drift from the live renderer.
// Data table; one entry per line shape. Length is inherent to the
// number of line shapes documented, not function complexity.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn output_doc_lines() -> Vec<LineDoc> {
    vec![
        LineDoc {
            kind: "build_ok",
            label: "BUILD OK / CHECK OK",
            grammar: "<MODE> OK [warnings=<N>] in <SECS>s [logs.stdout=<P> logs.stderr=<P>]",
            notes: "MODE is BUILD or CHECK. warnings is emitted only when >0.",
            fields: vec![
                FieldDoc {
                    name: "mode",
                    ty: "enum",
                    optional: false,
                    description: "BUILD or CHECK.",
                },
                FieldDoc {
                    name: "warnings",
                    ty: "u64",
                    optional: true,
                    description: "Count of `warning:` lines on cargo stderr.",
                },
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Wall-clock seconds, one decimal.",
                },
            ],
            examples: vec![
                Summary::BuildOk {
                    mode: SubcommandKind::Build,
                    warnings: 0,
                    elapsed_secs: 12.3,
                }
                .render(),
                Summary::BuildOk {
                    mode: SubcommandKind::Check,
                    warnings: 2,
                    elapsed_secs: 4.1,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "build_failed",
            label: "BUILD FAILED / CHECK FAILED",
            grammar: "<MODE> FAILED in <SECS>s [<diag1> | <diag2> | ...]",
            notes: "Diagnostics list capped at 6, joined with ' | '. Empty list yields '[no error lines captured]'.",
            fields: vec![
                FieldDoc {
                    name: "mode",
                    ty: "enum",
                    optional: false,
                    description: "BUILD or CHECK.",
                },
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Wall-clock seconds.",
                },
                FieldDoc {
                    name: "diagnostics",
                    ty: "list<string>",
                    optional: false,
                    description: "Up to 6 cargo stderr lines starting with `error[` or `error: `.",
                },
            ],
            examples: vec![
                Summary::BuildFailed {
                    mode: SubcommandKind::Build,
                    errors: vec!["error[E0599]: no method named `foo` found".into()],
                    elapsed_secs: 6.2,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "test_ok",
            label: "TEST OK",
            grammar: "TEST OK total=<N> passed=<N> failed=<N> ignored=<N> in <SECS>s [<runner>]",
            notes: "runner is `nextest` or `legacy` depending on the test path used.",
            fields: vec![
                FieldDoc {
                    name: "total",
                    ty: "u64",
                    optional: false,
                    description: "passed + failed + ignored.",
                },
                FieldDoc {
                    name: "passed",
                    ty: "u64",
                    optional: false,
                    description: "",
                },
                FieldDoc {
                    name: "failed",
                    ty: "u64",
                    optional: false,
                    description: "Always 0 for TEST OK.",
                },
                FieldDoc {
                    name: "ignored",
                    ty: "u64",
                    optional: false,
                    description: "",
                },
                FieldDoc {
                    name: "runner",
                    ty: "enum",
                    optional: false,
                    description: "`nextest` or `legacy`.",
                },
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Wall-clock seconds.",
                },
            ],
            examples: vec![
                Summary::TestOk {
                    runner: TestRunner::Nextest,
                    total: 2092,
                    passed: 2081,
                    failed: 0,
                    ignored: 11,
                    elapsed_secs: 18.4,
                }
                .render(),
                Summary::TestOk {
                    runner: TestRunner::Legacy,
                    total: 50,
                    passed: 50,
                    failed: 0,
                    ignored: 0,
                    elapsed_secs: 3.0,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "test_failed",
            label: "TEST FAILED",
            grammar: "TEST FAILED total=<N> passed=<N> failed=<N> ignored=<N> [failures=[<name1>, <name2>, ...]] in <SECS>s [<runner>]",
            notes: "failures list capped at 8 names. Names match the runner's identifier scheme.",
            fields: vec![
                FieldDoc {
                    name: "total",
                    ty: "u64",
                    optional: false,
                    description: "passed + failed + ignored.",
                },
                FieldDoc {
                    name: "failures",
                    ty: "list<string>",
                    optional: true,
                    description: "Up to 8 test identifiers; absent when no names were captured.",
                },
                FieldDoc {
                    name: "runner",
                    ty: "enum",
                    optional: false,
                    description: "`nextest` or `legacy`.",
                },
            ],
            examples: vec![
                Summary::TestFailed {
                    runner: TestRunner::Nextest,
                    total: 12,
                    passed: 11,
                    failed: 1,
                    ignored: 0,
                    failures: vec!["my_crate::tests::my_test".into()],
                    elapsed_secs: 12.0,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "test_build_failed",
            label: "TEST BUILD-FAILED",
            grammar: "TEST BUILD-FAILED in <SECS>s [<diag1> | <diag2> | ...] [<runner>]",
            notes: "Emitted when cargo failed to compile before any test ran. Diagnostics capped at 6.",
            fields: vec![
                FieldDoc {
                    name: "diagnostics",
                    ty: "list<string>",
                    optional: false,
                    description: "Up to 6 cargo stderr lines starting with `error[` or `error: `.",
                },
                FieldDoc {
                    name: "runner",
                    ty: "enum",
                    optional: false,
                    description: "`nextest` or `legacy`.",
                },
            ],
            examples: vec![
                Summary::TestBuildFailed {
                    runner: TestRunner::Nextest,
                    errors: vec!["error[E0599]: no method named `foo`".into()],
                    elapsed_secs: 6.2,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "clippy_ok",
            label: "CLIPPY OK",
            grammar: "CLIPPY OK warnings=<N> in <SECS>s",
            notes: "warnings is always emitted, even when zero, to distinguish from BUILD OK.",
            fields: vec![FieldDoc {
                name: "warnings",
                ty: "u64",
                optional: false,
                description: "Count of `warning:` lines on stderr.",
            }],
            examples: vec![
                Summary::ClippyOk {
                    warnings: 0,
                    elapsed_secs: 4.2,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "clippy_failed",
            label: "CLIPPY FAILED",
            grammar: "CLIPPY FAILED warnings=<N> in <SECS>s [<diag1> | ...]",
            notes: "Diagnostics list contains up to 6 errors (preferred) or warnings if no errors are present.",
            fields: vec![
                FieldDoc {
                    name: "warnings",
                    ty: "u64",
                    optional: false,
                    description: "Total warning count.",
                },
                FieldDoc {
                    name: "diagnostics",
                    ty: "list<string>",
                    optional: false,
                    description: "Up to 6 stderr lines.",
                },
            ],
            examples: vec![
                Summary::ClippyFailed {
                    warnings: 4,
                    diagnostics: vec!["warning: unused import: `std::io`".into()],
                    elapsed_secs: 4.1,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "timeout",
            label: "TIMEOUT",
            grammar: "TIMEOUT after <SECS>s (limit <LIMIT>s) -- child killed",
            notes: "Emitted instead of any other summary when --timeout fires. Process exits 124.",
            fields: vec![
                FieldDoc {
                    name: "secs",
                    ty: "f64",
                    optional: false,
                    description: "Elapsed wall-clock seconds at the moment the child was killed.",
                },
                FieldDoc {
                    name: "limit_secs",
                    ty: "u64",
                    optional: false,
                    description: "The --timeout value.",
                },
            ],
            examples: vec![
                Summary::Timeout {
                    elapsed_secs: 600.0,
                    limit_secs: 600,
                }
                .render(),
            ],
        },
        LineDoc {
            kind: "heartbeat",
            label: "HEARTBEAT (stderr, not a summary)",
            grammar: "[heartbeat <SECS>s stdout=<N> stderr=<N> +<N> since-last [latest=\"<excerpt>\"]]",
            notes: "Emitted on stderr while the child runs. Not a summary line. The excerpt is truncated to 120 chars with an '...' suffix.",
            fields: vec![
                FieldDoc {
                    name: "secs",
                    ty: "f64 (zero decimals)",
                    optional: false,
                    description: "Elapsed wall-clock seconds since spawn.",
                },
                FieldDoc {
                    name: "stdout",
                    ty: "u64",
                    optional: false,
                    description: "Cumulative stdout line count.",
                },
                FieldDoc {
                    name: "stderr",
                    ty: "u64",
                    optional: false,
                    description: "Cumulative stderr line count.",
                },
                FieldDoc {
                    name: "since-last",
                    ty: "u64",
                    optional: false,
                    description: "Lines seen since the previous heartbeat reset.",
                },
                FieldDoc {
                    name: "latest",
                    ty: "string",
                    optional: true,
                    description: "Most recent non-blank line, quoted, truncated to 120 chars.",
                },
            ],
            examples: vec![
                r#"[heartbeat 30s stdout=12 stderr=4 +6 since-last latest="Compiling foo v0.1.0"]"#
                    .into(),
            ],
        },
    ]
}
