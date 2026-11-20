# Ticket 178 — The test guard variable means one thing

**Blocked by:** none

## Why

Found on 2026-11-21. `BETTER_OS_TEST_SYSTEMCTL_GUARD` keeps tests and sandbox
runs away from the person's own user manager, shell settings and removal
hooks. Three places take a non-empty value as a log file path and append every
command they did not run to it (`crates/manager-platform/src/user_units.rs`
about lines 316 to 345, `shell_extensions.rs` about 257 to 270,
`removal_hooks.rs` about 95), and three read it as an on/off flag
(`crates/monitor-gui/src/startup.rs` about 561,
`crates/manager-gui/src/pages/touchpad/mod.rs` about 823,
`crates/manager-cli/src/main.rs` about 998). AGENTS.md says "Tests set
`BETTER_OS_TEST_SYSTEMCTL_GUARD`" without saying the value is a path, so
setting it to `1` makes Better Manager write a file named `1` into whatever
folder it was started from. The untracked `1` at the repository root came
from that.

## What to build

- The guard is a flag: any non-empty value turns it on, and nothing is written
  because of it. The record of what was not run moves to its own variable
  that names a file (absolute paths only; a relative value is refused with a
  message), read in one place all six callers use.
- AGENTS.md and the test helpers say which variable does what.

## Done when

- Tests: the flag alone writes nothing; the log variable writes where it
  names; a relative log path is refused.
- No file named after the flag's value appears when a window is started with
  the flag set.
