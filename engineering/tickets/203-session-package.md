# Ticket 203 — GDM starts the Better OS session from Better OS's own session package

**Spec:** better-os-session.md S1, S4 ("The session": what it is, the environment, the process table, how units bind)
**Blocked by:** 201

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

- [ ] Unit tests for the environment, the import and unset lists, the target
      ordering rules, and the shortcut table naming only shipped programs.
- [ ] In ticket 201's VM, the package installed by hand: GDM lists Better OS,
      the session starts with every unit of the table active, every Better OS
      unit of the installed components active, `XDG_CURRENT_DESKTOP` and
      `DCONF_PROFILE` as the spec says in a terminal and in a D-Bus-activated
      service; logging out stops all of them; logging in to GNOME afterwards
      shows none of the session's variables in `systemctl --user
      show-environment`. Notes and screenshots in
      `/home/tim/Developer/better-os-evidence/203/`.
- [ ] A GSettings write in the session lands in `~/.config/dconf/betteros`
      and GNOME's `~/.config/dconf/user` is byte-for-byte unchanged.
- [ ] fmt, clippy on stable, the crates' tests, `verify-deb.sh`, and the
      license inventory check pass.
