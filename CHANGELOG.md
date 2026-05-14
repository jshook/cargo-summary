# Changelog

All notable changes to this project are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Library API at `src/lib.rs` exposing the `Summary` enum, the
  parser functions (`summarize_test_nextest`, `summarize_test_legacy`,
  `summarize_build`, `summarize_clippy`), `SubcommandKind`,
  `CargoVersion`, `relativize_against` / `relativize_to_cwd`, and
  the output-documentation primitives. Downstream tools can depend
  on the crate as a library and reuse the parsers directly. The
  binary now imports from the library; both targets ship in the
  same crate.
- Doctests on every public library item, covering the typical
  usage shape.
- Defensive tests: malformed nextest JSON, empty inputs, diagnostic
  caps, multi-byte heartbeat truncation, missing-value CLI parse
  errors, out-of-range exit codes, unicode paths, and
  cross-reference checks between the subcommand docs and the
  summary kinds they reference.
- Per-subcommand documentation in `--describe-output[-json]`. Each
  summarizable subcommand now lists the exact cargo invocation used
  and the summary `kind`s it can produce.
- Explicit subcommand registry: `build`, `check`, `test`, `clippy`
  are summarized; any other cargo subcommand (e.g. `run`, `doc`,
  `fmt`, `bench`, custom external subcommands) is forwarded raw
  with inherited stdio. `--wrap-unknown` opts unknown subcommands
  into heartbeat / timeout / log capture without a summary.
- Cargo and `cargo-nextest` version detection, cached at first use
  and exposed via `--version-info`.
- `--describe-output`, `--describe-output-json`, and
  `--describe-output-schema` flags. The first two are produced by
  the same renderer the live tool uses, so documentation cannot
  drift from emitted output. The third emits a JSON Schema (draft
  2020-12) for the JSON description, identified by the URN
  `urn:cargo-summary:output-doc-schema:v1`.
- `AGENTS.md` at the repo root describing the tool for AI coding
  assistants (vendor-neutral convention).

### Changed

- Declared MSRV bumped from `1.85` to `1.88`. The previous claim
  was inaccurate: the code uses let-chains (`if let X = y && ...`),
  which stabilized in Rust 1.88. Verified by building against
  `rustc 1.88.0`.
- The `summarizable_subcommands` field in `--describe-output-json`
  was replaced by a richer `subcommands` array; each entry has
  `name`, `cargo_invocation`, `summary_kinds`, and `notes`. The
  JSON Schema was updated accordingly (URN unchanged at v1, since
  no document conforming to v1 has been published yet).
- Log capture suffix in summary lines is now
  `logs.stdout=<P> logs.stderr=<P>` (was `logs=<P> <P>`), to
  disambiguate paths on workspaces whose paths contain spaces.
- Log paths in the summary are now rendered relative to the
  process CWD by default. Pass `--absolute-log-paths` for the old
  absolute-path behavior (useful when the summary is consumed from
  a different working directory).
- `TEST BUILD-FAILED` summaries now include the `[nextest]` /
  `[legacy]` runner tag for symmetry with `TEST OK` / `TEST FAILED`.
- ASCII-only output: em-dash and ellipsis replaced with `--` and
  `...` so legacy Windows code pages render summaries correctly.
- Workspace-root probe now also recognizes a `[package]` Cargo.toml
  as a stopping point, not only a `[workspace]` one. Single-crate
  projects no longer fall through to the CWD-based fallback.
- Internal: every summary line shape now flows through a single
  `Summary` enum + `render()` method, so the on-the-wire format
  has one source of truth.

## [0.1.0] - 2026-05-14

Initial release.

### Added

- `cargo summary <subcommand>` wrapper that emits a single-line
  status (`BUILD OK`, `TEST FAILED`, `CLIPPY OK`, …) for cargo
  `build` / `check` / `test` / `clippy` invocations.
- `cargo-nextest` integration for `test` mode when available, with
  parsing of the line-delimited libtest-json event stream and
  automatic fallback to plain `cargo test` parsing. `--no-nextest`
  forces the legacy path.
- `--timeout SECS` — kill the child after a deadline and exit `124`.
- `--heartbeat SECS` / `--heartbeat-lines N` — emit periodic
  liveness lines on long-running invocations; either trigger resets
  both counters.
- `--passthrough` (`-p` / `-v`) — relay cargo's stdout/stderr
  verbatim alongside the summary.
- `--quiet-summary` — suppress the trailing summary line.
- Persistent stdout/stderr log capture under
  `<workspace>/target/cargo_summary/<mode>-<ts>.{stdout,stderr}.log`.

[Unreleased]: https://github.com/jshook/cargo-summary/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jshook/cargo-summary/releases/tag/v0.1.0
