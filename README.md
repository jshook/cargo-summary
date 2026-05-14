# cargo-summary

A small wrapper around `cargo` (and `cargo-nextest`, when present)
that produces a single-line status for build / test / clippy / check
invocations. Designed for environments — CI pipes, AI assistants,
remote logs — where the full cargo output is wasted noise and a
machine-parseable one-liner is what you actually want.

Invocable two ways once installed on `PATH`:

```sh
cargo summary test --workspace
cargo-summary test --workspace        # equivalent
```

## Output

```
TEST OK total=2092 passed=2081 failed=0 ignored=11 in 18.4s [nextest] logs=…stdout.log …stderr.log
BUILD OK warnings=2 in 12.3s logs=…
CLIPPY FAILED warnings=4 in 4.1s [warning: unused import …]
TEST BUILD-FAILED in 6.2s [error[E0599]: no method named `foo`…]
TEST FAILED total=N passed=N-1 failed=1 ignored=0 failures=[my_test] in 12s
TIMEOUT after 600.0s (limit 600s) — child killed
```

Exit codes match the wrapped cargo invocation; timeouts exit `124`.

## Features

- **Single-line summary** with normalized fields, regardless of
  cargo / libtest version drift.
- **`cargo-nextest` integration** for `test` mode when available —
  parses the line-delimited libtest-json event stream instead of
  regexing libtest text output (more robust, faster). Falls back to
  plain `cargo test` parsing automatically if nextest isn't on
  `PATH`. Force the legacy path with `--no-nextest`.
- **Heartbeats** for long-running invocations:
  - `--heartbeat SECS` fires after N seconds without progress
  - `--heartbeat-lines N` fires after N output lines
  - Either trigger resets both counters; the heartbeat shows
    elapsed time, line totals, and the most recent non-blank line
- **Timeout** (`--timeout SECS`) — kills the child and exits with
  `124`.
- **Passthrough** (`--passthrough` / `-p` / `-v`) — relays cargo's
  raw stdout/stderr verbatim alongside the summary, for
  troubleshooting.
- **Persistent log capture** — every invocation writes the full
  stdout and stderr to `<workspace>/target/cargo_summary/<mode>-<ts>.{stdout,stderr}.log`
  so a failed run can be re-inspected without re-running.

## Usage

```sh
cargo-summary [tool-flags] <cargo-subcommand> [cargo args...]

Tool flags (must precede the cargo subcommand):
    -p, --passthrough           Relay cargo output verbatim
    -v                          Alias for --passthrough
    --timeout SECS              Kill the child after SECS (exit 124)
    --heartbeat SECS            Emit a liveness line after SECS of silence
    --heartbeat-secs SECS       Alias for --heartbeat
    --heartbeat-lines N         Emit a liveness line after every N output lines
                                (fires on whichever heartbeat condition trips first)
    --no-nextest                Force `cargo test` for `test` mode even when
                                cargo-nextest is installed
    --quiet-summary             Suppress the trailing summary line
    -h, --help                  Print this help
```

## Examples

```sh
# Standard test run
cargo summary test --workspace

# Long build with heartbeat every 30 s
cargo summary --heartbeat 30 build --release --workspace

# Force passthrough to see compile errors live
cargo summary -p test -p my-crate failing::module

# CI: 10-minute cap, heartbeat every 60 s
cargo summary --timeout 600 --heartbeat 60 test --workspace
```

## Installation

From source:

```sh
git clone https://github.com/...your-repo.../cargo-summary
cd cargo-summary
cargo install --path .
```

For the test-mode JSON path, install `cargo-nextest`:

```sh
cargo install cargo-nextest --locked
```

(The wrapper auto-detects nextest at runtime; if it's not installed,
it falls back to `cargo test` parsing transparently.)

## Status

Pre-publication. Used in-house. Not yet on crates.io — the
`cargo-summary` name is unclaimed at the time of writing; the plan
is to claim it once the wrapper's behavior has stabilized in real
use.

## License

Apache-2.0.
