# Ticket 177 — CI never passes a test that did not run

**Blocked by:** none

## Why

Found on 2026-11-21: tests that need a program return early with "skipping"
when it is missing, and still count as passed. CI had no `dconf-service`, so
ticket 207's private-bus tests never ran there until its worker made them
fail under `CI=true`. The GJS extension tests
(`crates/window-attention-service/tests/extension.rs`,
`crates/touchpad-session/tests/extension.rs`, which assert AGENTS.md's bounds
on the shell extensions) still skip quietly, because CI does not install
`gjs`. Other `*_or_skip!` helpers need only `dbus-daemon`, which CI has.

## What to build

- One helper every such test uses: a missing prerequisite is a skip on a
  developer's machine and a failure when `CI=true`.
- The CI image installs what the tests need (`gjs` at least; list each
  prerequisite in the workflow with the tests that need it).
- A check in CI that the test log contains no skip line.

## Done when

- Removing a prerequisite from the CI install step turns CI red.

## Notes

- The helper is the crate `crates/test-prerequisites`. It has no
  dependencies at all, so every crate's tests take it as a dev-dependency
  without a cycle or a heavy edge (`better-core`'s tests could too), and
  `defaults-platform`'s `test-support` feature takes it as an optional
  dependency for `PrivateDconf::start_or_skip`. `require!(prerequisite,
  result)` and `require_that!(prerequisite, condition)` return from the test
  with a `better-os-test-skipped:` line on a developer's machine and panic
  when `CI` is set to anything but empty, `false`, or `0`.
- Every skip site moved onto it: the `bus_or_skip!`/`world_or_skip!` macros and
  let-else skips in awake-service, awake-tray, files-gui, hardware-platform,
  launcher-platform, manager-daemon (boot_watch, dbus_service, drives,
  first_call, hardware, keyboard_lighting), manager-gui (tests, first_call,
  portal, keep_awake), monitor-service, monitor-tray, storage-service,
  touchpad-gesture-service, touchpad-platform, window-attention-service; the
  two GJS extension tests; `PrivateDconf::start_or_skip`; and the skips that
  were not about a program: files-gui's zh_TW GVfs catalog test and its
  not-root undo test, files-platform's not-root folder-size test, and
  files-operations' sparse-file and extended-attribute tests (two of which
  printed nothing, or "not exercised", and passed).
- `crates/test-prerequisites/tests/one_skip_rule.rs` fails when any crate
  prints a `skipping:`/`skipped:`/"not exercised" message itself or reads the
  `CI` variable itself.
- CI: the install step adds `gjs` and lists each prerequisite with the tests
  that need it. `gjs -m` stops at the first `resource:///org/gnome/shell`
  import, which no machine outside a shell has; on this machine, which has no
  Meta, Shell, or Clutter typelib on GI's path either, that is the error both
  extensions report, so CI needs no shell typelib. The test step became
  "Tests, and none of them skipped": `cargo test --workspace -- --show-output`
  (a passing test's output is captured and otherwise never reaches the log),
  then a grep that fails on `better-os-test-skipped:` or a line starting
  `skipping:`/`skipped:`. The helper's own tests call a non-printing core and
  its panic message leaves the marker out, so they do not trip it.
- Why removing a prerequisite turns CI red: GitHub Actions sets `CI=true`, and
  `test_prerequisites::tests::a_missing_program_fails_under_ci` shows the
  helper panics under CI on a missing program. Run by hand here with `gjs`
  taken off `PATH`, window-attention-service's `the_extension_is_valid_javascript`
  printed the skip line and passed without `CI`, and failed with
  `CI=true` ("CI must run this test, and it needs gjs: …").
- Found, not part of this ticket: `packaging/verify-deb.sh` skips desktop
  entry validation when `desktop-file-validate` is missing, and the package
  job does not install `desktop-file-utils`; whether the runner image has it
  anyway was not checked.
