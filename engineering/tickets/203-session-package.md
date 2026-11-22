# Ticket 203 — GDM starts the Better OS session from Better OS's own session package

**Spec:** better-os-session.md S1, S4 ("The session": what it is, the environment, the process table, how units bind)
**Blocked by:** 201
**Blocked:** the key bindings for Super+A (the owner's decision S3, ticket 210), Alt+Tab (ticket 211), and the overview (ticket 212), and the theme's corner radius (ticket 148 has not decided the window radius); everything else is built and checked (Notes)

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
  wants it, through `better-os-session-autostart.target`, which only the
  launcher starts (the second review below).
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
Zorin OS 18.1 (the live ISO, checked against help.zorin.com's SHA-256, run
in the review round with the package of this branch): the closure added 80
packages, removed and changed none, and took exactly the 14 from System76;
GDM listed Better OS (screenshot 40); the session ran the target, its six
units, Tracker, and every component unit, with the spec's environment in
the user manager, a terminal (41), and the D-Bus-activated portal; a
GSettings write went to `betteros`; Print opened Better Screenshot (42); the
Zorin Desktop login after it showed GNOME's own environment (43). Records:
`/home/tim/Developer/better-os-evidence/203/zorin/`. This run found that
`better-os-session.target` wanting `xdg-desktop-autostart.target` without
`Before=` ordered it after that target (a target is ordered after what it
wants), a cycle through `graphical-session.target` that systemd broke by
dropping every autostart entry; with `Before=xdg-desktop-autostart.target`
the entries ran (`nm-applet`, `touchegg`, update-notifier, Déjà Dup) and
the journal had no cycle (unit test and `verify-deb.sh` hold it). The first
Ubuntu run had the same cycle unnoticed, so ticket 201's autostart list,
taken with its stand-in that had the `Before=`, is the one to trust.

### What is left, and on what

- Super+A: the owner's decision S3, then ticket 210.
- Alt+Tab, Alt+Shift+Tab, Super+Tab: ticket 211 ships the switcher.
- Super+W, XF86LaunchA: ticket 212 ships the overview.
- The theme's corner radius: ticket 148 has not said the window radius.
  Writing COSMIC's theme also means shipping the theme configuration's whole
  default directory (the one-directory rule above).
Each binding goes into the session's `defaults` together with its
`system_actions` command, and its line leaves `WAITING` in
`crates/session-launcher/tests/shortcuts.rs`.

### The review of 2026-11-22, and Better Screenshot

Review: `/home/tim/Developer/better-os-evidence/203/review-2026-11-22.md`.
VM records of this round: `/home/tim/Developer/better-os-evidence/203/review-round/`
and screenshots 20 to 32 beside NOTES.md.

- **P1, a launcher that dies.** Children get `PR_SET_PDEATHSIG`
  (`session_launcher::process`, test: a child dies with the thread that
  started it). The launcher writes a record of what it changed
  (`session_launcher::record`); `better-os-session-guard.service` watches
  the launcher and stops the target (and `graphical-session.target` when no
  other session binds it) when it ends, and its `ExecStopPost=` puts the
  environment back; `better-os-session-leftovers.service` does the same in
  any later session, the launcher before its own start. A leftover value is
  not taken for the person's own (`environment::without_leftovers`, test).
  VM: the launcher re-executed after an update, then `kill -9`: within 8 s
  the compositor, XWayland, and the locker were gone (so the parent-death
  signal survived the exec), every unit and `graphical-session.target`
  inactive, the record taken, the user manager without any session variable
  (`crash4/`); the GNOME login after showed GNOME's own environment, a
  D-Bus-activated GNOME Text Editor had `DCONF_PROFILE` naming the shipped
  `betteros-default` (`user-db:user`), and a write with its environment
  landed in `~/.config/dconf/user` while `betteros` kept its hash
  (`crash2/`, `t203-gnome-after-kill.txt`). Found on the way and fixed: a
  target `BindsTo=` the guard went down when the package's postinst
  restarted the guard during an update, taking the session's units and
  environment with it; now the target only wants the guard, and the guard's
  `ExecStopPost=` acts only when the session is over (`record::session_over`,
  test). An update in a running session then kept every unit running.
- **P2, D-Bus activation.** At the end `DCONF_PROFILE` is set there to a
  value dconf treats as its default (`dconf::default_profile`, tests; dconf
  0.40's `dconf_engine_profile_open` read: an empty value and `user` do not
  work on Ubuntu, which has no `user` profile). VM: the session bus is
  `dbus-daemon`; after a clean Better OS logout and a GNOME login on the
  same user manager, GNOME Text Editor started by D-Bus activation had
  `DCONF_PROFILE=/usr/share/dconf/profile/betteros-default` (`clean/`).
- **P2, Better Launcher.** `better-launcher` is in the session's `Depends`
  (ticket 205 carries it as a companion); the shortcut test now accepts a
  Better OS program only from the session package or its `Depends`.
- **P2, upstream drift.** `packaging/session/cosmic/upstream.tsv` records the
  two upstream files with versions and hashes;
  `guest/check-shortcut-upstream.sh` passed against the installed closure in
  the VM and fails with instructions on a changed file (tried on an altered
  copy); ticket 204's closure check is told to run the same comparison.
- **P3.** The re-exec sets `FD_CLOEXEC` again (in `adopt` and after a failed
  exec; VM: the socket's flags read `02004002` after a re-exec) and never
  execs with an ending queued (`resume::may_restart`, test). AGENTS.md
  names the launcher as the fourth update mechanism. The target and its
  units carry `ConditionPathExists=` on the record (unit tests, verify-deb;
  VM journal: "skipped because of an unmet condition check" when started
  without one). An unreadable user manager takes back only the session's
  own (test). The derived defaults are in the shipped notice and
  copyleft-review.md. The caller check compares device and inode (tests,
  including a same-path file that is not the installed one).
- **im-launch.** In the session `im-launch` does nothing: it acts only with
  `IM_CONFIG_PHASE=1`, which `/etc/profile.d/im-config_wayland.sh` sets in a
  login shell, and GDM starts the launcher without one. With the user
  manager's `LANG=zh_TW.UTF-8` its autostart unit ran and exited 0, and
  exactly one `fcitx5`, the unit's, ran (`t203-imlaunch*.txt`). GDM kept
  `LANG=en_US.UTF-8` for the session although AccountsService said
  `zh_TW.UTF-8` (no Chinese language pack in the VM), so the session leader
  itself never ran with the Chinese locale.
- **dconf system databases.** Ubuntu 24.04 (VM) and Zorin OS 18.1 (this
  installed machine, read only) ship no `/etc/dconf/profile/user`; both have
  only IBus's `ibus` profile. The shipped `betteros` profile is therefore
  exactly the session's; with an administrator's `user` profile holding
  `system-db` lines, the launcher writes a profile with them and points
  `DCONF_PROFILE` at it (`dconf::session_profile`, tests; not run in a VM).
- **Better Screenshot** (ticket 253, merged): the binary, its portal file,
  D-Bus activation file, and desktop entry, the GStreamer plugins, Print and
  the three keys, and the portal line are in the package. The desktop
  entry's `OnlyShowIn=BetterOS;` was dropped: `desktop-file-validate`
  refuses an unregistered desktop name, and `NoDisplay=true` already keeps
  it out of every list. VM results are in ticket 253's notes.

### The second review of 2026-11-22

Review: `/home/tim/Developer/better-os-evidence/203/review-2-2026-11-22.md`.
VM records: `/home/tim/Developer/better-os-evidence/203/review-2/` (its
NOTES.md) and screenshots 44 and 45.

- **P2, leftovers under GNOME.** The VM run the review asked for (guard
  stopped, launcher killed, linger, GNOME login) found that the leftovers
  unit never ran: the dead session left `graphical-session-pre.target`
  active, so GNOME Shell started with `DCONF_PROFILE=betteros`. The unit is
  now also wanted by and before GNOME's `gnome-session-pre.target`. It
  touches only what is still the session's
  (`environment::leftover_restore`, `lifecycle::put_back_leftovers`, tests):
  the session-only values, the session's entry anywhere in `XDG_DATA_DIRS`
  (GNOME's login had put its own in front), and the input method values
  beside the leftover `DCONF_PROFILE`; never a display. It sets activation
  before it unsets, because `dbus-daemon --systemd-activation` passes the
  activation update on to the user manager (seen in the VM; the fake now
  does the same). Run 3, from the rebuilt package: the user manager and
  GNOME Shell had GNOME's values only, `graphical-session.target` and
  Tracker stayed.
- **`held_by_another`** counts a binding unit with a queued start job
  (`lifecycle::holds`, test). In run 3 the leftovers unit ran two seconds
  before `gnome-session.target` was reached and left
  `graphical-session.target` running.
- **The activation `DCONF_PROFILE` file** ships in `better-manager`, which
  the session depends on: activation keeps the path after the session
  package goes, and a missing named profile is dconf's null configuration.
- **dconf's runtime profile** is `$XDG_RUNTIME_DIR/dconf/profile` (dconf
  0.40.0 `engine/dconf-engine-profile.c` line 33, used at lines 243-258);
  `dconf::RUNTIME_PROFILE`, test.
- **Autostart and Tracker under GNOME.** A condition skips only its own
  unit, so they moved to `better-os-session-autostart.target`, which only
  the launcher starts (unit tests, `verify-deb.sh`). VM: autostart entries
  ran in the session with no ordering cycle; `systemctl --user start
  better-os-session.target` under GNOME skipped every unit and never loaded
  the autostart target. Tickets 203 and 205 say so now.
- **P3s.** The leftovers unit has `TimeoutStartSec=15s`. An unreadable unit
  state during the guard's restart counts as a session still running. The
  guard exits with failure when it could not stop the target. Stop signals
  are watched through a polled `signalfd` and never consumed, so one that
  comes during the restart in place stays pending across the exec
  (`process::pending_stop_signal`; tests: a watched signal stays pending,
  a pending SIGTERM crosses `Command`'s exec). A record keeps only the
  session's names when read (test). `install-session.sh` runs
  `check-shortcut-upstream.sh` against the installed closure and an altered
  copy (VM: `same` twice, the altered copy refused). Ticket 253's two key
  defects are open items there; their cause is Better Screenshot's focus
  handling, not the session's data. Ticket 205's exclusion names the guard
  and the leftovers unit.
- **Not rerun:** Zorin OS 18.1 (its ISO was deleted for disk space).
