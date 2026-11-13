# Ticket 105 — After an update, everything runs the new version

**Blocked by:** none · **Status:** ready

## Why

The owner, 2026-11-14: after a Better OS app is updated, it only behaves
like the new version once it is restarted by hand. Updating should restart
what it replaced, background services included, so no part of Better OS
keeps running the old program next to new files and settings.

What restarts today:

- A component's systemd user units are enabled and restarted by Better
  Manager when it installs, updates, or restores the component
  (`manager-platform/src/user_units.rs`, `enable_and_restart`). Ticket 102
  and 97 turn the Awake and Monitor tray icons into user units, so they are
  covered when updated through Better Manager.
- Nothing restarts an open application window. After an update it keeps
  running the deleted old binary (`/proc/<pid>/exe` ends in `(deleted)`),
  and can talk to a restarted service with a newer protocol (ticket 97 bumped
  Better Monitor's).
- Nothing restarts anything when the packages are updated outside Better
  Manager (`apt upgrade`, Software Updater, unattended upgrades).
- `better-manager-daemon` (the system service) is reloaded by its
  maintainer script, but whether it is restarted on upgrade has to be
  checked.

## What to build

- **One rule, in AGENTS.md.** After any update of a Better OS package,
  however it was installed, every Better OS process of that package runs the
  new program within a few seconds: services, tray icons, and open windows.
- **Services and trays, whatever updated them.** Each package's maintainer
  scripts restart its system units on upgrade (`deb-systemd-invoke
  try-restart` or the debhelper equivalent) and, for user units, ask every
  logged-in user's manager to restart them
  (`systemctl --user -M <uid>@ try-restart` or `systemd-run --user` per
  user, only units that are running, never starting a unit the person
  turned off). Better Manager keeps doing its own restart, and the two
  never fight (try-restart is idempotent). A user unit that is new in the
  upgraded package (ticket 102's `better-awake-tray.service`, ticket 97's
  `better-monitor-tray.service`, which replace autostart entries) is enabled
  and started for each logged-in user when the component was enabled
  before, so an upgrade with `apt` does not leave the person without the
  tray until Better Manager next runs.
- **Windows restart themselves and come back where they were.** Every app
  watches its own program file (`/proc/self/exe` replaced, or an inotify on
  the installed path). When it changes, the app saves its view state (open
  folder, repository, page, tab, selection, scroll, window size and place,
  and any text being typed such as a commit message), starts the new
  program with that state, and exits; the new window replaces the old one in
  place. If the app is in the middle of something that cannot be carried
  over (a running file operation, a Git operation, an open dialog with
  unsaved input that has no saved form), it waits until that finishes and
  then restarts, showing a small "Updated — restarting when this finishes"
  note meanwhile. A shared `better-ui` helper does the watching, state
  hand-off, and re-exec, so every app uses the same code.
- **Better Manager restarting itself.** When Better Manager updates itself
  it finishes the transaction, records the result, restarts into the new
  version, and shows the result page.
- **Visible, not silent.** After a restart the window shows "Updated to
  {version}" for a few seconds (the same short-lived notice as ticket 103's
  toasts), never a modal.

## Acceptance

- [ ] In a disposable VM or container with a session: update each component
      both through Better Manager and with `apt`, with its window open and
      its services running; afterwards no Better OS process has a
      `(deleted)` executable, every window is back with its state, and a
      service the person had turned off stays off. Record the checks.
- [ ] Unit tests for the state save and restore of each app, the "wait until
      the operation finishes" rule, and the maintainer scripts' per-user
      restart (with a fake systemctl).
- [ ] Tests never touch the person's real systemd user manager or windows.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      `build-deb.sh` + `verify-deb.sh` for all nine packages, and the
      license check pass.
