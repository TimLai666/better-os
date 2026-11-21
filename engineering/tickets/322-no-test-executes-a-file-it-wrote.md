# Ticket 322 — No test executes a file it has just written, checked by a guard

**Spec:** AGENTS.md, working rules ("A test never executes a file it has just written")
**Blocked by:** none

## Why

`crates/files-gui/tests/plugins.rs` wrote a script and ran it; under CI the
run failed now and then with ETXTBSY (reported as "not found"), and turned
pull request 38 red on 2026-11-22 for a change that never touched it. The
helper now has a child write the file. About twenty other test files write an
executable file (`fs::write` beside `from_mode(0o755)`), and nothing stops the
next one.

## What to build

- Read every test that writes an executable (start from
  `grep -rln 'from_mode(0o7[05]5)' crates/*/tests crates/*/src`) and, where
  the test or the code under test then runs that file, change it to one of the
  two forms AGENTS.md allows: run it as `/bin/sh <script>`, or reach a
  checked-in fixture through a symbolic link the test makes; or have a child
  write it, as `plugins.rs` does now, where the file must be run by its own
  name.
- A shared helper in `test-prerequisites` (or the crate the workspace's test
  helpers live in) for "a program a test needs to exist", so the rule has one
  home.
- A guard test in the style of `one_skip_rule.rs` that fails when a test
  writes a file and sets an executable mode on it outside that helper.

## Done when

- [ ] Every such test is changed or shown not to run what it wrote, listed in
      the notes.
- [ ] The guard test fails on the old `plugins.rs` helper and passes now.
- [ ] `cargo test --workspace --no-fail-fast` run twenty times in a row with
      no failure from this cause.
