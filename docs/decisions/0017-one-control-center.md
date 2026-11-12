# ADR 0017: One control center inside Better Manager

## Status

Proposed on 2026-11-12 for the owner's direction of the same day (ticket 76,
direction 6). Accepted when the owner approves ticket 83. It replaces the
"separate apps" arrangement for Better Awake, Better Touchpad, and Better
Defaults, and it amends ADR 0007 by adding a second daemon interface for
hardware.

## Context

Better OS has four windows that each adjust part of the same computer: Better
Manager (with Better Defaults inside it as a sidebar page), Better Awake's
settings window `awake-gui`, and Better Touchpad's window `better-touchpad`.
Each has its own sidebar, its own language and appearance settings, and its own
idea of a page. The 2026-11-12 audit found the same faults repeated across
them (choices that don't show what is selected, notices that never go away,
navigation that loses its labels, language and appearance that are set
three times). The owner decided that Better OS stops growing small apps and
that Better Manager becomes one control center laid out like Windows 11
Settings, also holding Better Patch's known-fault fixes and hardware controls.

Hardware controls on the owner's machine (read on 2026-11-12, see
`design/control-center-spec.md` section 11) split into two kinds. Power mode,
display brightness, and keyboard backlight already have an unprivileged path
the desktop itself uses: power-profiles-daemon, whose polkit action allows an
active session, and gsd-power's `Power.Screen` and `Power.Keyboard`
interfaces. The battery charge limit and the fan mode are sysfs files only
root can write, and nothing on the machine offers a session-level path to
them.

## Decision

### One window

`better-manager` is the only settings window. Its pages are Home, System,
Touchpad, Keep awake, Apps, Default apps, Fixes, and Settings, as the spec
lays out. One language setting and one appearance setting cover every page.

The window opens a given page with `better-manager --page <page>`, and a
second invocation hands the page to the running window over the session bus
name `io.betteros.Manager` instead of opening a second window.

A page whose component is not installed stays in the navigation and says so,
with an Install button. A page whose component is installed but older than
the page needs says so, with an Update button. Touchpad and Keep awake are
such pages; their settings stay tied to the component because the component
owns the background service, the capture that removal restores, and the
health checks.

### The components stay; their windows go

| Component | Package keeps | Package loses | What opens the page |
| --- | --- | --- | --- |
| Better Awake | `better-awake-service` and its user unit, `awake-tray` and its autostart entry, the icon | `/usr/bin/awake-gui` | `better-awake.desktop` stays, `Exec=better-manager --page keep-awake`; the tray runs the same command |
| Better Touchpad | `better-touchpad-gestured` and its user unit, the GNOME Shell adapter extension, the safe-mode entry, the icon | the settings window | `better-touchpad.desktop` stays, `Exec=better-manager --page touchpad` |
| Better Defaults | (no package; it was already inside Better Manager) | the Defaults sidebar page | Default apps |
| Better Patch | (never shipped) | — | Fixes |

`better-touchpad` stays as a command so its documented `--safe-mode` and
`--normal-mode` keep working from a text console. It no longer links GPUI:
with no argument it runs `better-manager --page touchpad`, with
`--safe-mode` or `--normal-mode` it writes or removes the marker and exits,
and anything else is refused as today.

The two kept desktop entries are search shortcuts, so a person who types
"保持清醒" or "touchpad" in the launcher still lands on the page. They set
`StartupNotify=false`, because the window that appears belongs to
`io.betteros.Manager` and a startup notification for the shortcut's own name
would never complete.

The page code lives in `manager-gui`, which gains dependencies on
`awake-ipc` (as a client), `touchpad-core`, `touchpad-platform`,
`touchpad-gestures`, and `touchpad-session`. Every decision a page draws is
made in a module with no GPUI dependency, as `manager-gui::model` and
`touchpad-gui::model` already do.

### On upgrade

- Updating Better Awake to the first release without `awake-gui` removes the
  binary through dpkg like any file a new package version no longer ships. A
  running `awake-gui` keeps running until closed. The service and the tray are
  restarted by the existing update path (`Update and Restore restart a unit
  that is already running`).
- The quick-session lengths and session defaults that `awake-gui` kept in
  `$XDG_STATE_HOME/better-awake/awake-gui-preferences.json` move into the
  Better Awake service, which imports that file once on its first start and
  then owns the values, so the tray and the Keep awake page read the same
  list. The file's language and appearance are not imported; the control
  center's own settings apply. The file is left in place, not deleted.
- Better Touchpad's configuration, capture, and safe-mode marker stay where
  they are (`~/.config/better-os/touchpad/`) and keep their formats, so a
  rollback to the previous package still reads them.
- Each new component manifest declares a dependency on the Better Manager
  release that carries its page, so the manager installs or updates itself
  first. A tray or shortcut that runs `better-manager --page` against an older
  manager would otherwise be refused (ticket 60) and open nothing.
- Rolling back Better Awake or Better Touchpad to a release that still ships
  its window brings that window back alongside the control center. Both read
  the same service or files, so nothing is lost; there are just two windows
  until the next update.

### Hardware writes

Unprivileged controls go through the desktop's own services, never through
the daemon: power mode through power-profiles-daemon, display brightness and
keyboard backlight through gsd-power on GNOME (UPower's `KbdBacklight` where
gsd-power is absent). Writing `platform_profile` directly is not allowed: the
daemon already drives it and the two would fight.

Root-only controls go through `manager-daemon`. This amends ADR 0007, whose
protocol version 1 has one interface and one polkit action:

- A second interface, `org.betteros.Hardware1`, on the existing bus name
  `org.betteros.Manager1` and object path, so there is still one privileged
  service, one package, and one activation file.
- One method per control and one polkit action per method:
  `SetChargeLimit(y)` behind `org.betteros.manager.set-charge-limit`, and
  `SetFanMode(s)` behind `org.betteros.manager.set-fan-mode`, both
  `auth_admin` for any and inactive sessions and `auth_admin_keep` for the
  active one. A new control is a new method and a new action, never a
  parameter of an existing one.
- Arguments are closed: a charge limit of 60, 80, or 100, a fan mode of `auto`
  or `full`. The daemon finds the device itself, writes, reads back, and
  returns what it read. No path, device name, or free text crosses the bus.
- Reads are not methods. Reading these files is unprivileged, and the window
  reads them directly as AGENTS.md allows.
- The unit keeps `ProtectKernelTunables=yes` and opens only the attribute
  files it writes with `ReadWritePaths=`. That this works under systemd 255 is
  shown in a VM before the change merges; if it does not, the protection is
  not dropped without a new decision.
- If the charge limit does not survive a reboot, a oneshot system unit
  restores the stored value at boot and does nothing else. Fan mode is never
  restored.

### Better Patch

Better Patch is a page of the control center, not a component and not a part
of Better Defaults. Defaults changes which program owns a setting; a fix
changes a broken setting back to a working one, and the two would have to
share every state word with different meanings. The engine is a new crate with
no GPUI dependency. Each entry is typed Rust in the repository with a test: a
detector, a plain-language description, a fix, a check that the fix worked,
and an undo. Nothing is fetched or supplied at run time, nothing runs through
a shell, and an entry that needs root is a new daemon method under the rule
above. The first two entries are the two faults ticket 76 records; neither
needs root.

## Consequences

- One window to build, translate, test, and theme instead of three; one
  navigation model and one set of states.
- Better Manager's binary grows by the touchpad and awake client code. The
  touchpad crates bring no new external package, since `manager-gui` already
  links `zbus`; ticket 84 confirms this against `Cargo.lock`.
- A person without Better Manager cannot change Keep awake or Touchpad
  settings. Better Manager is installed by `install.sh` before anything else,
  so this only happens if someone removes it on purpose.
- The awake service's protocol gains requests for the quick-session lengths
  and defaults, so the page refuses to talk to an older service and offers to
  update it.
- `AGENTS.md`'s privileged-mutation rule is unchanged; the daemon simply has
  more than one interface.

## Alternatives not taken

- **A separate hardware daemon.** Its own unit could drop
  `ProtectKernelTunables` without touching dpkg's sandbox, but it would be a
  second root service, package, and policy file to review, and the owner's
  direction names `manager-daemon`. Kept as the fallback if `ReadWritePaths`
  cannot open the two files.
- **Keeping the windows and linking them from Better Manager.** Leaves three
  language and appearance settings and three navigation models, which is what
  the owner asked to end.
- **Hiding a page until its component is installed.** A person reading about
  Better Awake would not find where it lives.
- **Keyboard backlight through the daemon.** Not needed: the desktop already
  writes it without root.
