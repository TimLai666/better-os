# Ticket 176 — What Better Defaults applied is put back when its component is removed

**Blocked by:** none

## Why

AGENTS.md: uninstalling removes everything the component added, and the
keybindings and defaults it set are restored to what they were. Ticket 207's
worker found on 2026-11-21 that `manager-core` never calls the defaults
engine on removal: the only uninstall-time restore is Better Touchpad's
before-removal step, and ticket 129 adds one for Better Files' activation
file. A component whose manifest declares other integrations (keybindings,
desktop settings, launcher entries) leaves them applied after it is gone.

## What to build

- Removing a component restores every integration the engine recorded as
  applied for it, through the engine's own restore (the changed-externally
  rule holds: a value the person changed afterwards is left and named), from
  whichever session the removal runs in (ticket 207: a record names its
  database).
- The same when the package is removed without Better Manager (apt, Software):
  the per-person step tickets 90 and 129 built for their own cases is the
  pattern; one mechanism for every component, not one copy per package.
- Disabling a component: decide and record whether its integrations are
  restored on disable and applied again on enable (ticket 129 decides this
  for the activation file; the rule should be one rule).
- Better Touchpad's own capture records which dconf database it was taken
  from, as ticket 207 did for the engine's records, so a restore from the
  other session reaches the right one.

## Done when

- Tests: remove from the same session, from the other session, with a value
  changed externally, with no session at all (pending, finished at the next
  start), and by apt in the container end-to-end check.

## Notes

- 2026-11-21, what exists now for this ticket to fold together (from ticket
  129, after ticket 90 landed). Better Files' before-removal and on-disable
  programs are the second and third special case after Better Touchpad's;
  none of the pieces below is general yet, and each is named so this ticket
  can take it in rather than copy it.
  - **The hook directories.** `/usr/lib/better-os/before-removal/<package>`
    (`manager_platform::removal_hooks::BEFORE_REMOVAL_DIRECTORY`,
    `before_removal_hook`) and `/usr/lib/better-os/on-disable/<package>`
    (`ON_DISABLE_DIRECTORY`, `on_disable_hook`). Both are found only in
    dpkg's file list, refused unless a root-owned regular file nobody else
    can write (`refuse_program`), run as the person by `ProcessHookRunner`
    for `BEFORE_REMOVAL_TIMEOUT` (30 s), never block, and read exit 3 as a
    warning and 4 as pending (`EXIT_NOT_RESTORED`, `EXIT_DEFERRED`), `--undo`
    (`UNDO_ARGUMENT`) to reverse. manager-core runs them in
    `exec.rs`' `run_removal_hooks` (before the daemon removes the package,
    `--undo` if it refuses) and `run_disable_hook` (on disable, `--undo` on
    enable). Shipped by `better-touchpad` (before-removal, its own touchpad
    capture), `better-manager` (before-removal, its fixes; run by its own
    maintainer scripts, ticket 90) and `better-files` (both directories, one
    binary `better-files-before-removal`).
  - **The engine side of Better Files' program.**
    `defaults_core::removal`: `put_back` restores the engine's record for
    every default a manifest declares (`restore_component`), removes an
    activation file that is byte for byte ours when there is no record
    (`defaults_platform::bus_activation::remove_own_files`, which also
    renames a kept-aside file back), and remembers what it gave back;
    `take_again` applies exactly that again; `RemovalState` keeps
    `<component>.pending` (put-back or take-back) and `<component>.given-back`
    under `$XDG_STATE_HOME/better-os/defaults-put-back`. Only
    `BETTER_FILES_MANIFEST` and `better_files_program` are Better Files' own;
    the rest takes any manifest.
  - **The startup pass that already walks every component.**
    `defaults_core::removal::finish_in_session`, called from
    `crates/manager-gui/src/app.rs` (the startup pass, skipped when
    `BETTER_OS_TEST_SYSTEMCTL_GUARD` is set), takes every manifest in the
    catalog and the record's standing (installed by record or dpkg, on or
    off) and finishes each pending put-back or take-back by it
    (`finish_one`), and runs the leftover pass for a component that is not
    installed. It is the place a general "restore on removal, re-apply on
    enable" lands for a person who had no session at the time.
  - **The account helper for removals without Better Manager.**
    `packaging/maintainer/accounts.sh` (`better_os_for_each_account STEP
    [ARG...]`: accounts from `getent` in the login range, plain names only,
    a home the mount table says is reachable, 30 s per step, 120 s in all)
    and `account-step.sh` (`better_os_usable_home`, `better_os_as_account`,
    `better_os_prefix_lines`), held in a shell variable by build-deb.sh's
    `account_block` and never written to a file. Three fragments use it:
    `autostart-overrides.sh` (Better Touchpad's postrm),
    `before-removal-accounts.sh` (Better Manager's prerm, postinst
    abort-remove and postrm, which run its before-removal program as each
    person) and `files-activation.sh` (Better Files' postrm, which removes
    the activation file and renames the kept-aside file back as each person;
    `packaging/tests/files-activation.test.sh`, and the container check in
    `packaging/test-daemon-e2e.sh`). `before-removal-accounts.sh` is the
    general shape: a package's own before-removal program run by its
    maintainer scripts. Better Files' postrm does not run its program that
    way yet; it only does the file-level part a postrm can do without the
    engine's record.
  What is still special: which package ships which program, Better Files'
  postrm doing a file-level subset rather than running its program, and
  Better Touchpad's capture outside the engine's record.
