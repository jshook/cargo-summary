# AGENTS.md — guidance for AI assistants

This document tells AI coding assistants what `cargo-summary` is,
when to reach for it, and how to consume its output reliably.
Conventions follow the emerging `AGENTS.md` pattern (a vendor-neutral
counterpart to `CLAUDE.md` / `.cursorrules` / `.windsurfrules`).

## What this tool is

`cargo-summary` is a thin wrapper around `cargo` that reduces a full
cargo invocation to a single concise summary line. It is designed
for environments where the full multi-thousand-line cargo output is
wasted context — CI pipes, log aggregators, and AI assistants that
need to know "did this build pass, and roughly how" without reading
every diagnostic.

It is *not* a replacement for cargo. It runs cargo under the hood
and never modifies the workspace. Exit codes match the wrapped
cargo invocation; a timeout fires exit code `124`.

## When an agent should use it

Reach for `cargo summary <subcommand>` instead of raw cargo when:

- You need a build/test status and don't intend to read the full
  output. Examples: post-edit verification, CI gating, "did my
  refactor break anything?" checks.
- You are running cargo in an environment with a strict context
  budget (an assistant conversation, a log channel with a size cap).
- You need a timeout or a heartbeat — `cargo` has neither built-in.
- You are about to capture cargo output for later inspection and
  want a stable file location: `cargo-summary` writes the full
  stdout/stderr under `target/cargo_summary/` automatically.

Reach for raw `cargo` when:

- You need to read the diagnostic detail live (interactive
  development).
- You are running a cargo subcommand `cargo-summary` does not
  summarize (`run`, `doc`, `fmt`, `bench`, custom external
  subcommands). In that case `cargo summary` falls through to raw
  cargo by default, so calling it costs nothing — but invoking
  cargo directly is clearer.

## Invocation

Two equivalent forms once on `PATH`:

```sh
cargo summary <subcommand> [cargo args...]
cargo-summary <subcommand> [cargo args...]
```

Subcommands `cargo-summary` knows how to summarize: `build`,
`check`, `test`, `clippy`. Anything else is forwarded raw (stdio
inherited; cargo's behavior is bit-for-bit unchanged) unless
`--wrap-unknown` is passed.

Useful flags:

| Flag | Effect |
| --- | --- |
| `--timeout SECS` | Kill the child after `SECS`; exit `124`. |
| `--heartbeat SECS` | Emit a liveness line every `SECS` of stdout/stderr silence. |
| `--heartbeat-lines N` | Emit a liveness line every `N` output lines. |
| `--passthrough` | Relay cargo output verbatim alongside the summary. |
| `--wrap-unknown` | Apply heartbeat/timeout/log capture to unknown subcommands. |
| `--absolute-log-paths` | Emit log paths in the summary as absolute. Default is CWD-relative. |
| `--no-nextest` | Force `cargo test` for test mode even when `cargo-nextest` is installed. |

For long-running invocations in agent loops, `--timeout` plus
`--heartbeat` are the two flags that matter most: they bound how
long the wrapper can be silent, which keeps the agent's polling
loop predictable.

## Output discovery (programmatic)

The output format is documented at runtime. An agent that wants to
parse summary lines should ask the binary for its current contract
rather than baking format strings into prompts.

| Command | Purpose |
| --- | --- |
| `cargo-summary --describe-output` | Human-readable grammar with examples. |
| `cargo-summary --describe-output-json` | Same content as a JSON document. |
| `cargo-summary --describe-output-schema` | JSON Schema (draft 2020-12) for the JSON document. |
| `cargo-summary --version-info` | Wrapper, cargo, and nextest versions. |

The JSON document carries a `schema` field with the URN
`urn:cargo-summary:output-doc-schema:v1`, identifying the schema
the document conforms to. Bumps to that URN signal breaking changes.

Each summary line has a stable `kind` identifier (`build_ok`,
`test_failed`, `timeout`, etc.). Pin to those identifiers, not to
the human-readable labels.

## Output shapes (quick reference)

The single line cargo-summary emits always matches one of these
shapes. Run `--describe-output` for full grammar; this is a quick
reference only.

```
BUILD OK [warnings=<N>] in <SECS>s [logs.stdout=<P> logs.stderr=<P>]
CHECK OK [warnings=<N>] in <SECS>s [logs...]
BUILD FAILED in <SECS>s [<diag1> | <diag2>] [logs...]

TEST OK total=<N> passed=<N> failed=0 ignored=<N> in <SECS>s [nextest|legacy] [logs...]
TEST FAILED total=<N> passed=<N> failed=<N> ignored=<N> [failures=[...]] in <SECS>s [runner] [logs...]
TEST BUILD-FAILED in <SECS>s [<diag1>] [runner] [logs...]

CLIPPY OK warnings=<N> in <SECS>s [logs...]
CLIPPY FAILED warnings=<N> in <SECS>s [<diag1>] [logs...]

TIMEOUT after <SECS>s (limit <LIMIT>s) -- child killed [logs...]
```

Heartbeats are emitted on **stderr**, are not summaries, and look
like:

```
[heartbeat 30s stdout=12 stderr=4 +6 since-last latest="Compiling foo v0.1.0"]
```

Full stdout/stderr is captured under
`<workspace-root>/target/cargo_summary/<mode>-<millis>.{stdout,stderr}.log`
so an agent can re-inspect a failed run by reading those files
without re-running cargo.

## Limitations

- **Format is not yet stable.** This is pre-1.0. The grammar may
  evolve; pin to `wrapper_version` in tooling that needs strict
  guarantees, and watch the `doc_version` / schema URN.
- **Parsing depends on cargo's output format.** Cargo and libtest
  occasionally tweak their text output between releases. The
  nextest path (`--message-format libtest-json`) is more robust
  than the legacy `cargo test` regex path.
- **Heartbeats and summaries are intermixed.** Heartbeats go to
  stderr; the final summary goes to stdout. Agents should separate
  streams or look for the `[heartbeat ...]` prefix.
- **Log file paths in the summary** are CWD-relative by default
  (e.g. `target/cargo_summary/build-X.stdout.log`). Pass
  `--absolute-log-paths` if you need absolute paths — useful when
  the summary is consumed from a different working directory than
  the one cargo-summary ran in. Paths may contain spaces on macOS
  / Windows workspaces; the `logs.stdout=` / `logs.stderr=`
  prefixes disambiguate, but a naive split by spaces will
  mis-tokenize — read each value up to the next ` logs.` or
  end-of-line.
- **No interactive support.** stdin is inherited only in raw mode;
  in wrapped mode cargo's stdout/stderr are piped, so anything
  reading TTY directly will not see one.
- **Cross-platform.** Linux / macOS / Windows supported. Output is
  ASCII-only so legacy Windows code pages do not mangle it.

## How to confirm cargo-summary is installed

```sh
cargo-summary --version-info
```

If that exits 0 and prints a version line, the wrapper is available.
If it is not, fall back to raw `cargo`; nothing in this repository
depends on the wrapper being present.
