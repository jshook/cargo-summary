# cargo-summary

[![CI](https://github.com/jshook/cargo-summary/actions/workflows/ci.yml/badge.svg)](https://github.com/jshook/cargo-summary/actions/workflows/ci.yml)

A small wrapper around `cargo` (and `cargo-nextest`, when present)
that produces a single-line status for build / check / test / clippy
invocations. Designed for environments — CI pipes, AI assistants,
remote logs — where the full cargo output is wasted noise and a
machine-parseable one-liner is what you actually want.

Two equivalent invocations once installed on `PATH`:

```sh
cargo summary test --workspace
cargo-summary test --workspace        # equivalent
```

Subcommands that cargo-summary knows how to summarize: `build`,
`check`, `test`, `clippy`. Any other cargo subcommand is forwarded
raw (stdio inherited) unless you pass `--wrap-unknown`.

## Output

```
BUILD OK warnings=2 in 12.3s logs.stdout=target/cargo_summary/build-...stdout.log logs.stderr=target/cargo_summary/build-...stderr.log
CHECK OK in 4.1s logs.stdout=target/... logs.stderr=target/...
TEST OK total=2092 passed=2081 failed=0 ignored=11 in 18.4s [nextest] logs.stdout=... logs.stderr=...
TEST FAILED total=12 passed=11 failed=1 ignored=0 failures=[my_test] in 12.0s [nextest] logs...
TEST BUILD-FAILED in 6.2s [error[E0599]: no method named `foo`] [nextest] logs...
CLIPPY OK warnings=0 in 4.2s logs...
CLIPPY FAILED warnings=4 in 4.1s [warning: unused import] logs...
TIMEOUT after 600.0s (limit 600s) -- child killed logs...
```

Log paths are relative to the current working directory by default
(pass `--absolute-log-paths` to emit absolute paths). The
underlying files always live at
`<workspace-or-package-root>/target/cargo_summary/<mode>-<millis>.{stdout,stderr}.log`.

Exit codes match the wrapped cargo invocation; timeouts exit `124`.

The format is documented at runtime — see [Output description](#output-description).

## Features

- **Single-line summary** with normalized fields, regardless of
  cargo / libtest version drift.
- **`cargo-nextest` integration** for `test` mode when available —
  parses the line-delimited libtest-json event stream instead of
  regexing libtest text output (more robust, faster). Falls back to
  plain `cargo test` parsing automatically if nextest isn't on
  `PATH`. Force the legacy path with `--no-nextest`.
- **Heartbeats** for long-running invocations:
  - `--heartbeat SECS` fires after N seconds of silence
  - `--heartbeat-lines N` fires after N output lines
  - Either trigger resets both counters; the heartbeat shows
    elapsed time, line totals, and the most recent non-blank line.
- **Timeout** (`--timeout SECS`) — kills the child and exits `124`.
- **Passthrough** (`--passthrough` / `-p` / `-v`) — relays cargo's
  raw stdout/stderr verbatim alongside the summary.
- **Persistent log capture** — every invocation writes the full
  stdout/stderr to
  `<workspace>/target/cargo_summary/<mode>-<ts>.{stdout,stderr}.log`
  so a failed run can be re-inspected without re-running.
- **Cargo / nextest version awareness** — detected once and exposed
  via `--version-info`. Version branches are added only where
  cargo's output format actually differs.
- **Cross-platform** — Linux, macOS, Windows. ASCII-only output,
  no POSIX signals, no shell invocations.

## Usage

```sh
cargo-summary [tool-flags] <cargo-subcommand> [cargo args...]
```

Tool flags (must precede the cargo subcommand):

| Flag | Effect |
| --- | --- |
| `-p`, `--passthrough` | Relay cargo output verbatim alongside the summary. |
| `-v` | Alias for `--passthrough`. |
| `--timeout SECS` | Kill the child after `SECS` (exit `124`). |
| `--heartbeat SECS` | Emit a liveness line after `SECS` of silence. |
| `--heartbeat-secs SECS` | Alias for `--heartbeat`. |
| `--heartbeat-lines N` | Emit a liveness line every `N` output lines. |
| `--no-nextest` | Force `cargo test` for test mode (skip nextest). |
| `--wrap-unknown` | Apply heartbeat/timeout/log capture even for cargo subcommands cargo-summary doesn't know how to summarize. |
| `--absolute-log-paths` | Render log paths in the summary as absolute. By default they are relative to the current working directory. |
| `--quiet-summary` | Suppress the trailing summary line. |
| `--describe-output` | Print the output-format grammar and exit. |
| `--describe-output-json` | Print the output-format description as JSON. |
| `--describe-output-schema` | Print the JSON Schema (draft 2020-12) for the JSON description. |
| `--version-info` | Print cargo-summary, cargo, and nextest versions. |
| `-h`, `--help` | Print help and exit. |

## Examples

```sh
# Standard test run
cargo summary test --workspace

# Long build with a heartbeat every 30 s
cargo summary --heartbeat 30 build --release --workspace

# Force passthrough to see compile errors live
cargo summary -p test -p my-crate failing::module

# CI: 10-minute cap, heartbeat every 60 s
cargo summary --timeout 600 --heartbeat 60 test --workspace

# Wrap an unknown subcommand (no summary, but heartbeat + logs apply)
cargo summary --timeout 300 --wrap-unknown doc --workspace

# Discover the output format programmatically
cargo summary --describe-output
cargo summary --describe-output-json | jq .
cargo summary --describe-output-schema
```

## Output description

Running cargo-summary against an unfamiliar codebase? The wrapper
documents its own output format. Three formats are available:

- `cargo summary --describe-output` — annotated human-readable
  grammar with examples for every line shape.
- `cargo summary --describe-output-json` — the same content as a
  structured JSON document, including a `schema` field that
  identifies the schema the document conforms to.
- `cargo summary --describe-output-schema` — the JSON Schema (draft
  2020-12) for that JSON document, so downstream tools can validate
  their parsers.

The JSON document and the human grammar share a single source of
truth: the same `Summary::render` function the live tool uses
produces every documented example. A test enforces that they cannot
drift.

## AI assistant guidance

[AGENTS.md](AGENTS.md) describes when an AI agent should reach for
`cargo summary` (versus raw `cargo`), how to parse the output
robustly, and the tool's current limitations. The format follows
the emerging vendor-neutral `AGENTS.md` convention so it works with
Cursor, Cline, Windsurf, Claude Code, and other agent tooling
without per-vendor duplication.

## Library API

The crate also ships a small Rust library exposing the parsers and
types behind the CLI:

```rust
use cargo_summary::{Summary, SubcommandKind, summarize_build};

let stderr: Vec<String> = vec!["warning: unused import".into()];
let summary = summarize_build(&stderr, true, 1.0, SubcommandKind::Build);
assert_eq!(summary.render(), "BUILD OK warnings=1 in 1.0s");
```

The public items are documented (run `cargo doc --open`) and have
doctests. The library is pre-1.0 — pin a patch version in
`Cargo.toml` if you depend on it.

## Installation

From crates.io:

```sh
cargo install cargo-summary
```

From source:

```sh
git clone https://github.com/jshook/cargo-summary
cd cargo-summary
cargo install --path .
```

For the test-mode JSON path, install `cargo-nextest`:

```sh
cargo install cargo-nextest --locked
```

(The wrapper auto-detects nextest at runtime; if it's not installed,
it falls back to `cargo test` parsing transparently.)

Minimum supported Rust version: **1.88** (edition 2024 + let-chains).

## License

Apache-2.0. See [LICENSE](LICENSE).
