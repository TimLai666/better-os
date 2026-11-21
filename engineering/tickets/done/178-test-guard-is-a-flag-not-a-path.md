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

## Notes

Built on 2026-11-21.

- `better_core::test_guard` is the one reading of both variables.
  `BETTER_OS_TEST_SYSTEMCTL_GUARD` is a flag (any non-empty value; empty or
  unset is off) and writes nothing. `BETTER_OS_TEST_GUARD_LOG` names an
  absolute file the commands held back are appended to; a relative value is
  refused, the refusal is said once on stderr and is part of every "is not
  run while … is set" message, and nothing is written. The systemctl runner,
  the gsettings runner, the before-removal runner, Better Monitor's
  `systemctl`, Better Manager's touchpad restore and the CLI's startup pass
  all read it.
- The one test that set the guard to a path (`manager-cli`
  `host_invocation.rs`) sets the flag and the log variable now. No script,
  CI step or spec set it to a path. AGENTS.md says which variable does what.
- Tests: `better-core` `test_guard::tests` (the flag alone writes nothing,
  the log writes where it names, a relative log is refused);
  `manager-cli` `the_guard_flag_alone_writes_nothing_where_the_command_was_started`.
- Window: Better Manager's window from the build directory, started with
  `BETTER_OS_TEST_SYSTEMCTL_GUARD=1` from an empty folder in a private
  headless GNOME Shell, left the folder empty
  (`/home/tim/Developer/better-os-evidence/175/manager-guarded.png`, log in
  `/tmp/claude-1000/w77/sandbox/run/log/inner.log`).
