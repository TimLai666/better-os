# Ticket 129 — Better Files can be the desktop's file manager

**Blocked by:** none

## Why

Ticket 89 found that the key Better Files' manifest names for "the system
file manager" (`/org/gnome/desktop/applications/file-manager`) does not exist
on GNOME 46. What GNOME actually calls for "Show in folder" and "Open
containing folder" is the `org.freedesktop.FileManager1` D-Bus service, which
Nautilus activates and Better Files already serves when no one else owns it.

## What to build

- Record in an ADR how Better Files becomes the default for
  `org.freedesktop.FileManager1` (a user-level D-Bus activation file that
  takes precedence over Nautilus's, and how a running Nautilus that already
  owns the name is handled), and how Restore puts Nautilus back.
- Retarget Better Files' manifest integration from the dead key to this, with
  the defaults engine's adapter, review wording, restore, and uninstall
  cleanup (the activation file is removed and the previous owner restored).
- Tests against a private session bus: after "Use Better OS defaults" a
  `ShowItems` call reaches Better Files; after Restore it reaches the fake
  Nautilus.

## Notes

- 2026-11-21, from ticket 143's COSMIC VM: COSMIC Files' desktop process takes
  `org.freedesktop.FileManager1` at login, and Better Files then fails to take
  the name. The ADR this ticket writes says what happens on a desktop where
  another program holds the name from the start of the session.
- 2026-11-21, from ticket 134: on GNOME 46 the manifest's
  `file-manager-tool-entry-point` reads `gsettings.no_such_key`, the dead key
  this ticket retargets.
- 2026-11-21, built and tested; one thing left. Done: ADR 0041 (how the
  default is held, a running owner left alone, Restore, removal, COSMIC);
  the manifest's `file-manager-service` (`user-service` kind and adapter,
  `boolean: true`, key `org.freedesktop.FileManager1`, `sign-out`), which
  every released manager already accepts; `defaults_platform::bus_activation`
  in the production adapter set; `better-files --service`; the Default apps
  page's words for the row in both languages; `better-files-before-removal`
  shipped as `/usr/lib/better-os/before-removal/better-files`; and
  `crates/defaults-core/tests/bus_activation.rs`, where real `ShowItems`
  calls on a private `dbus-daemon` reach the stand-in Nautilus before, the
  stand-in Better Files after "Use Better OS defaults", Nautilus after
  Restore and after the removal program, a running Nautilus keeps answering
  until it leaves, and a foreign user file comes back byte for byte.
- 2026-11-21, review fixes (items 7 to 17 of the review): a window a
  `--service` process opened comes back after an update (the hand-off is
  taken, `restart::first_window`); a call is answered once its window is
  open and the name is given back before a service process leaves; removal
  by any means takes the activation file away (the package's postrm,
  `packaging/maintainer/files-activation.sh`, for every account; Better
  Manager's start for an account it could not reach); no session is exit 4
  with a pending put-back, not "restored"; the removal program remembers
  what it gave back and `--undo` takes exactly that again; turning Better
  Files off runs the same program from `/usr/lib/better-os/on-disable/` and
  turning it on runs its `--undo` (manager-core); only an `owned-by:` reading
  waits for sign-out, any other file is changed externally and left alone;
  ` (deleted)` is taken off the owner's executable; the file is synced
  before its rename, a foreign file is kept aside whole so its mode comes
  back, and empty `dbus-1` directories go; the page names the declaring
  component and "what the system starts on its own", and the result page
  too; `bus_or_skip!` fails in CI (`CI` set; the `rust` job installs
  `dbus`); ADR 0041 says what was verified about `dbus-daemon` and that
  `dbus-broker` was not.
- Seen in a sandbox (2026-11-21): the worktree's `manager-gui --page
  default-apps`, real execution, in a headless sway with its own HOME, XDG
  directories, private session bus (`dbus-run-session`), no system bus,
  `BETTER_OS_TEST_SYSTEMCTL_GUARD` set and a state file holding only Better
  Files' record. In English and zh-TW: before (Background service / 背景服務
  "What the system starts on its own" / 「系統原本啟動的程式」, Not default),
  the review ("After: Better Files", takes effect after signing out), the
  result, the list after "Use Better OS defaults" (Default / 目前為預設), and
  Restore's review and result ("What the system starts on its own" again),
  in `/home/tim/Developer/better-os-evidence/129/`. The activation file was
  written in the sandbox's data directory only (the person's
  `~/.local/share/dbus-1` does not exist) and Restore removed it with its
  empty directories. `en-restore-review` was retaken and
  `zh-TW-restore-review` added after the second review, on that round's
  build.
- Checks run on the final tree: `cargo fmt --all --check`; `cargo +stable
  clippy -D warnings --all-targets` and `cargo test --no-fail-fast` for
  storage-platform, drive-mounts, storage-service, manager-daemon,
  defaults-platform, defaults-core, manager-platform, manager-core,
  better-core, files-gui, manager-gui, manager-cli and touchpad-platform
  (1840 passed, 0 failed); `bash packaging/tests/files-activation.test.sh`;
  `bash -n` / `sh -n` on every packaging script changed. Not run:
  `packaging/build-deb.sh` and `verify-deb.sh` (no package was built),
  `shellcheck` (not installed here; CI runs it).
- 2026-11-21, second review fixes: Better Manager's start finishes a
  pending put-back or take-back by the record, so a component turned off
  with no session is put back at the next start and one turned on again is
  taken again (`defaults_core::removal::finish_in_session`, for every
  manifest, not by name); `take_again` keeps what it gave back until the
  write succeeds and marks a take-back pending without a session; Better
  Manager's leftover pass renames a kept-aside foreign file back
  (`remove_own_files`); the name is released without dropping the
  connection, so a call already received is still answered; the on-disable
  rules are in AGENTS.md and `engineering/architecture.md`, with a test that
  a group-writable on-disable program is refused; ticket 176 has a note; ADR
  0041 cites Ubuntu 22.04's `dbus-user-session` 1.12.20-2ubuntu4.1.
- Checks run on the second-review tree: `cargo fmt --all --check`; `cargo
  +stable clippy -D warnings --all-targets` and `cargo test --no-fail-fast`
  for storage-platform, drive-mounts, storage-service, manager-daemon,
  defaults-platform, defaults-core, manager-platform, manager-core,
  better-core, files-gui, manager-gui, manager-cli and touchpad-platform
  (1901 passed, 0 failed, 8 ignored); `bash
  packaging/tests/files-activation.test.sh`.
- 2026-11-21, after ticket 90 landed: Better Files' postrm goes through the
  shared account library (`account_block files-activation`, so
  `better_os_for_each_account` with its `getent` login range, name check,
  mount-table test, 30 s per person and 120 s in all); no copy of a helper
  is left in `files-activation.sh`. Root checks only that the home is the
  account's own; the account, under `setsid` with a cleared environment and
  no terminal, removes the file only when it is a regular file it owns and
  byte for byte ours (a link, FIFO or device is never opened), renames the
  person's kept-aside file back when nothing is in its place (and so does
  `remove_own_files` now, the same rule), removes empty directories, and
  asks its bus to reload. Nothing runs on upgrade, and nothing can fail
  dpkg. `files-activation.test.sh` drives the library and build-deb.sh's own
  postrm (hostile names through a stand-in `getent`, a FIFO in place of the
  file, a step cut off at its time limit, `TMPDIR=/nonexistent`, upgrade);
  CI already lints and runs it beside the account tests.
  `packaging/test-daemon-e2e.sh` installs `better-files` in the container
  (its GStreamer Depends are now in the image), reinstalls it (nothing
  changes), removes it under a time limit, and asserts Better OS's file is
  gone, the person's file is back with uid 1000 and mode 640, and that the
  step ran as that uid (it only touches files its own uid owns).
- Checks run on this tree (main merged, ticket 90 and 147 in): `cargo fmt
  --all --check`; `cargo +stable clippy -D warnings --all-targets` and
  `cargo test --no-fail-fast` for storage-platform, drive-mounts,
  storage-service, manager-daemon, defaults-platform, defaults-core,
  manager-platform, manager-core, better-core, files-gui, manager-gui,
  manager-cli and touchpad-platform (1926 passed, 0 failed, 8 ignored);
  `files-activation`, `account-scripts`, `before-removal-accounts` and
  `autostart-overrides` shell tests; `sh -n` and `bash -n` on every changed
  script; `generate-third-party-notices.sh --check`. Not run here:
  `shellcheck` (not installed; read by hand, CI runs it), `build-deb.sh` and
  `verify-deb.sh` (no package built), and the container check (no usable
  Docker here): its Better Files part has never run, and its first run is
  CI's.
- **Left:**
  - The container check's Better Files removal has its first run in CI.
  - The `dbus-broker` check: on a session whose bus is `dbus-broker` (in
    Ubuntu 24.04's `universe`, not the default), prove that it searches
    `$XDG_DATA_HOME/dbus-1/services` before `/usr/share/dbus-1/services` and
    reads the directory again on `ReloadConfig`, by running
    `crates/defaults-core/tests/bus_activation.rs` against a
    `dbus-broker-launch` private bus. Not possible here: `dbus-broker` is
    not installed and nothing may be installed on this machine, and there is
    no container or VM image. Ubuntu 22.04 is `dbus-daemon` 1.12 (ADR 0041);
    its watching of a user services directory is still unrun (the adapter
    calls `ReloadConfig` either way).
  - A real GNOME "Show in folder" opening Better Files, and a real Nautilus
    giving the name up: the owner's, on the machine, once a build with this
    is installed.

**Blocked:** the container check's first run is CI's (no usable Docker
here); the owner's own "Show in folder" on their GNOME desktop; a
`dbus-broker` session for the search-order check.
