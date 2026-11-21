# Ticket 203 — GDM starts the Better OS session from Better OS's own session package

**Spec:** better-os-session.md S1, S4 ("The session": what it is, the environment, the process table, how units bind)
**Blocked by:** 201
**Blocked:** the key bindings for Super+A (the owner's decision S3, ticket 210), Alt+Tab (ticket 211), the overview (ticket 212), and Print (ticket 253, pull request 32), and the theme's corner radius (ticket 148 has not decided the window radius); everything else is built and checked (Notes)

## Why

`cosmic-session` cannot be reused (its component list is fixed in code), so
Better OS starts the session itself: a session file, a launcher, a systemd
user target, and the session's configuration of COSMIC's daemons.

## What to build

- **Crate `session-launcher`**, binary `/usr/libexec/better-os/session/start`:
  sets the environment in the spec's table, execs `cosmic-comp` with itself
  as the program to run once the compositor is up; in that mode imports the
  variables into the systemd user manager and D-Bus activation, starts
  `better-os-session.target`, waits for the compositor to end, stops the
  target, and unsets what it imported. Every rule (the variable list, what is
  imported, the order) in a module with tests; the process edges thin.
  Logout is a method on `org.betteros.Session1` (`Logout`, owned by the
  launcher, callable by the session's own surfaces checked by executable) that
  stops the target and ends the compositor.
- **Units**: `better-os-session.target` (`BindsTo=graphical-session.target`,
  `Wants=graphical-session-pre.target xdg-desktop-autostart.target`), and one
  unit per COSMIC daemon the spec's table names (`cosmic-settings-daemon`,
  `cosmic-osd`, `cosmic-idle`, `cosmic-bg`, the locker as ticket 201 found it,
  the portal through D-Bus activation), Fcitx 5, and `tracker-miner-fs-3`
  where installed, each `PartOf=` and `WantedBy=better-os-session.target`.
- **The session file** `/usr/share/wayland-sessions/better-os.desktop`
  (Name 「Better OS」, `DesktopNames=BetterOS;COSMIC`), the dconf profile
  `/usr/share/dconf/profile/betteros`, `betteros-portals.conf`
  (`default=cosmic;gtk;`, Secret to gnome-keyring), and the session's defaults
  for COSMIC's configuration under `/usr/share/better-os/session/` (or, if
  ticket 201 found `cosmic-config` does not read them there, through ticket
  206's adapter with capture and restore): key bindings with Super, Super+Space,
  Super+A, Alt+Tab, and the workspace keys pointing at Better OS's programs
  (each named program must exist, a test checks the list against the package
  set), COSMIC's GTK-file syncing off if ticket 201 found it writing GNOME's
  files, the theme's corner radius from ticket 148's style.
- **Package `better-os-session`** built by `packaging/build-deb.sh`, with
  `THIRD-PARTY-LICENSES.md`, `Depends` on the 14 COSMIC packages and the Fcitx
  5 packages; `verify-deb.sh` checks the session file's `Exec`, the target, the
  profile, and that no unit of it is `WantedBy=graphical-session.target`.
- **Every existing Better OS unit** is checked to start in the session through
  `graphical-session.target` and to stop at logout, with no unit edited.

## Done when

- [x] Unit tests for the environment, the import and unset lists, the target
      ordering rules, and the shortcut table naming only shipped programs.
- [x] In ticket 201's VM, the package installed by hand: GDM lists Better OS,
      the session starts with every unit of the table active, every Better OS
      unit of the installed components active, `XDG_CURRENT_DESKTOP` and
      `DCONF_PROFILE` as the spec says in a terminal and in a D-Bus-activated
      service; logging out stops all of them; logging in to GNOME afterwards
      shows none of the session's variables in `systemctl --user
      show-environment`. Notes and screenshots in
      `/home/tim/Developer/better-os-evidence/203/`.
- [x] A GSettings write in the session lands in `~/.config/dconf/betteros`
      and GNOME's `~/.config/dconf/user` is byte-for-byte unchanged.
- [x] fmt, clippy on stable, the crates' tests, `verify-deb.sh`, and the
      license inventory check pass.

## What ticket 201 found that this ticket builds on

Ticket 201 ran a hand-made stand-in of this package in VMs on Ubuntu 24.04
and Zorin OS 18.1 (`packaging/tests/session-vm/guest/session-files.sh`,
evidence in `/home/tim/Developer/better-os-evidence/201/`). The spec's
process table now carries the facts; these change what is built here:

- **The locker is not a user unit.** `cosmic-greeter` in a user session is the
  locker and finds its logind session by its own PID; started as a systemd
  user unit it exits with `org.freedesktop.login1.NoSessionForPID`. The
  launcher starts it as its own child inside the session's scope. Check the
  same for `cosmic-osd`'s polkit agent before making it a unit (ticket 201 ran
  it as a child of the session, where it worked; as a unit is untested).
- **Do not set `GTK_IM_MODULE`** (the spec's environment table): with it,
  snap GTK applications lose Chinese input.
- **`betteros-portals.conf` needs `org.freedesktop.impl.portal.Secret=gnome-keyring;`**
  or no Secret portal is offered.
- **`tracker-miner-fs-3` is `WantedBy=gnome-session.target`**: the session
  target `Wants=` it.
- **`system_actions`**: COSMIC's defaults name `cosmic-app-library`,
  `cosmic-launcher`, `cosmic-screenshot`, `cosmic-term`, and
  `cosmic-workspaces`, none installed; the session's defaults name shipped
  programs (screenshot: ticket 253).
- **The launcher ends its children**: a logout left the session scope in
  `stop-sigterm` with `cosmic-settings-daemon` alive after 34 s once.
- **A greeter already running did not list a new session file** until GDM
  restarted (Ubuntu); ticket 205 checks the real install-then-log-out path.

## Notes (2026-11-22)

### What is built

- `crates/session-launcher`, installed as `/usr/libexec/better-os/session/start`:
  `environment` (the spec's table, the import list, `GTK_IM_MODULE` withheld,
  each changed variable put back to its value before the session),
  `compositor` (the session socket's message), `lifecycle` (import before the
  target; stop the target, wait for `graphical-session.target`, restore last),
  `children` (the locker, restart limit, SIGTERM then SIGKILL), `callers`
  (`org.betteros.Session1.Logout` from `/usr/bin/better-manager`;
  `com.system76.CosmicSession.Exit` from `/usr/bin/cosmic-osd`, which is what
  Super+Shift+Escape's confirmation calls), `resume` (the launcher re-executes
  itself after an update of its package, keeping its children), `process`
  (children start with no signal blocked). 29 unit tests, 7 on the shortcut
  table, 10 on the units.
- **Changed from the ticket's text, with the spec updated first:**
  - `cosmic-comp` is the launcher's child through `COSMIC_SESSION_SOCK`, not
    exec'd with the launcher as its program: a program to run puts it in
    kiosk mode, which turns every key binding off (`src/config/mod.rs` at
    `6968c5d`). Ticket 201's stand-in had that, unnoticed.
  - The session's units are not `WantedBy=better-os-session.target` (that
    needs an enable step per person); the target names each with `Wants=`,
    and they have no `[Install]`. `user-units.sh` now never enables or counts
    a static unit, so an upgrade can never start one under GNOME (regression
    test in `packaging/tests/maintainer-scripts.test.sh`).
  - `cosmic-osd` is a unit: its polkit agent works from outside the session
    scope (VM, screenshot 09). Only the locker is the launcher's child.
  - `cosmic-config` takes a configuration's defaults from one directory, the
    first under `XDG_DATA_DIRS` that has it, not key by key as the spec said;
    the session therefore ships the whole
    `com.system76.CosmicSettings.Shortcuts/v1/` (`defaults` based on
    cosmic-comp's GPL-3.0-only `keybindings.ron`, marked so, and
    `system_actions`), and no `custom`.
  - COSMIC's GTK file syncing is left as is: ticket 201 found no file under
    `~/.config/gtk-3.0` or `gtk-4.0` written on either base.
- Package `better-os-session` (Ubuntu 24.04 targets only), its
  `verify-deb.sh` block, CI's package list for 24.04, release-packaging.md.

### Checked

The VM run is in `/home/tim/Developer/better-os-evidence/203/NOTES.md` with
17 screenshots and the records: Ubuntu 24.04, every item of the second
"Done when" and the GSettings item, the launcher's update in a running
session, and GTK on XWayland typing Zhuyin without `GTK_IM_MODULE`. Three
defects found there are fixed in this change (kiosk mode, the compositor's
displays left in the user manager, children inheriting the blocked SIGTERM).
**Not run:** Zorin OS 18.1 with the package (ticket 201 ran the closure and a
stand-in there; the package's files do not differ by base).

### What is left, and on what

- Super+A: the owner's decision S3, then ticket 210.
- Alt+Tab, Alt+Shift+Tab, Super+Tab: ticket 211 ships the switcher.
- Super+W, XF86LaunchA: ticket 212 ships the overview.
- Print and the screenshot keys, the portal line, the binary and its
  Depends: ticket 253 (pull request 32, not merged); each place is marked
  "Ticket 253 adds" and ticket 253's "What to build" lists them.
- The theme's corner radius: ticket 148 has not said the window radius.
  Writing COSMIC's theme also means shipping the theme configuration's whole
  default directory (the one-directory rule above).
Each binding goes into the session's `defaults` together with its
`system_actions` command, and its line leaves `WAITING` in
`crates/session-launcher/tests/shortcuts.rs`.
