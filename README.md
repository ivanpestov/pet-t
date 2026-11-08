# t

A small Linux CLI countdown timer that can also record each finished
interval as a time-tracking entry.

The timer runs entirely in the foreground. It shows the start time,
the remaining time and the planned finish time, sends a desktop
notification when the countdown ends, and appends the completed period
to a Ledger timeclock file when an account is given.

## Features

- Countdown from a positive whole number of minutes.
- Plain timer mode without any time-tracking setup.
- Optional hierarchical account, recorded in a Ledger file.
- Automatic ledger entry on natural completion, Ctrl+C or system
  suspend.
- Sleep detection through systemd-logind on the system D-Bus; the
  sleep delay inhibitor is held until the open period is closed.
- Desktop notification on natural completion, with a bell and a
  terminal message as fallback.
- Shell completion for accounts through the external `ledger` command.

## Requirements

- Linux with systemd-logind and an available system D-Bus. The timer
  refuses to start when it cannot subscribe to `PrepareForSleep` or
  acquire the sleep delay inhibitor.
- The `date` command, used for local time and ledger timestamps.
- `notify-send` for desktop notifications. Without it the timer still
  works and falls back to a bell and text on stderr.
- `ledger` (3.x) in `PATH` only for shell completion of accounts.
- Rust 1.85 or newer to build from source (edition 2024).

## Build and install

```sh
cargo build --release
```

The binary is `target/release/t`. To install it for the current user:

```sh
ln -s "$PWD/target/release/t" ~/.local/bin/t
```

```sh
cargo build --release \
  && ln -sf "$PWD/target/release/t" ~/.local/bin/t \
  && t --complete > ~/.local/share/bash-completion/completions/t
```

## Usage

```text
t <minutes>
t <account> <minutes>
```

Examples:

```sh
t 25                    # plain 25-minute timer
t work:project 90       # timer that records a period under work:project
```

Arguments are validated before the ledger is touched. Invalid input
prints the usage line and exits with code 2.

While running, the timer renders one line in the form:

```text
12:34 => 25:00 => 12:59
```

The fields are the local start time, the remaining time rounded up to
whole minutes, and the finish time. The line is refreshed once a minute
on the same terminal line. The last field shows the planned finish time
while the timer runs and the actual finish time once it stops. The final
line ends with a newline.

`Ctrl+C` stops the timer, closes an open period with the actual local
time and exits with code 130. A system suspend closes the period and
exits with code 0. Neither case sends a "time is up" notification.

## Accounts

An account is an optional first argument. It is a hierarchical category
made of one or more segments separated by `:`. Each segment follows the
rules of a regular Rust identifier:

- The first character is `_` or a Unicode `XID_Start` character.
- The remaining characters are Unicode `XID_Continue` characters.
- Unicode is allowed, so `работа:проект` is a valid account.
- Rust keywords, a lone `_`, raw identifiers such as `r#type`, empty
  segments, spaces and other punctuation are rejected.
- Accounts are case-sensitive.

## Time tracking

Time tracking is enabled only when an account is passed. The ledger file
is taken from the `LEDGER_FILE` environment variable; if it is unset or
empty, the timer exits with code 1 before starting. The file is created
if missing, but parent directories are not created.

New entries are appended, so existing history is preserved. A missing
final newline is repaired before appending. The timer refuses to start
if the last non-empty entry opens a period that was never closed; it
never fixes such a period automatically.

Each tracked period adds two entries:

```text
i 2026/11/01 12:34:56 work:project
o 2026/11/01 12:36:00
```

`i` starts a period and carries the account. `o` ends it. Timestamps are
local time with second precision, and every write is flushed to disk
before the countdown relies on it.

Only one writing timer per ledger file is supported. Plain timers
without an account are not restricted.

## Shell completion

`t --complete` without arguments prints a ready-to-save bash completion
script to stdout and exits with code 0. Store it in the user completion
directory:

```sh
t --complete > ~/.local/share/bash-completion/completions/t
```

Bash Completion loads that directory automatically. The script registers
a function with `complete -F`; it calls the hidden protocol

```text
t --complete <current-word> <previous-word>
```

which prints one account per line, taken from `ledger accounts`,
filtered by the current word as a case-sensitive byte prefix. It
suggests accounts only for the first positional argument after `t`;
other positions produce no output. Failures, including a missing
`ledger` command or `LEDGER_FILE`, produce empty output and exit code 0.

If an older registration such as `complete -F _todo t` exists in a
startup file, remove it: it shadows the script above.

## Exit codes

| Code | Meaning                                                        |
| ---- | -------------------------------------------------------------- |
| 0    | Timer completed, or suspended with the period closed correctly |
| 1    | Startup or runtime failure, or the period could not be closed  |
| 2    | Invalid command-line arguments                                 |
| 130  | Interrupted by Ctrl+C                                          |

A notification failure never changes the exit code. A failed final
ledger write is reported on stderr, still attempts the notification and
returns 1.

## Architecture

The crate is split into three layers with dependencies pointing inward:

- `src/domain` holds pure rules: accounts, timer status, ledger entries,
  local time. It has no I/O dependencies.
- `src/application` orchestrates the timer scenario and declares ports
  for the clock, the ledger and notifications.
- `src/adapters` implements those ports with files, `date`, D-Bus,
  `notify-send`, Ctrl+C and the CLI.
- `src/main.rs` composes the layers, parses arguments and maps the
  outcome to an exit code.

The library crate is an internal implementation detail of the binary and
does not promise a stable API. The layering decision is recorded in
[ADR 0001](docs/adr/0001-clean-architecture.md).

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The test suite includes process-level integration tests with stubbed
`date` and `notify-send` commands. Some of them run real one-minute
timers, so a full run takes several minutes. Tests that need a real
system suspend remain a manual laptop check.

Domain terms are defined in [GLOSSARY.md](GLOSSARY.md). Repository
conventions for agents live in [AGENTS.md](AGENTS.md).
