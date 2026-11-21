# Ticket 295 — The tray shows the power mode, and its battery picture shows the battery's state

**Blocked by:** none

## Why

The owner said on 2026-11-21, asked whether Better Monitor's tray icon should
change with the battery's state: the tray should gain a display of which
power mode the machine is in now (the platform profile: quiet, balanced,
performance), and the part that changes with the battery's state should be
the battery picture.

Ticket 170 gave every battery picture in the windows one rule
(`hardware-core`'s charge state), and left the panel icon as a bar or a
number in one colour, so on battery, charging and holding at the limit look
the same there.

## What to build

- The tray icon's battery reading draws ticket 170's states: on battery,
  charging, holding at the limit, full, plugged in and not charging, unknown,
  each recognisable at the panel's size in light and dark panels, from the
  same rule and never a second mapping.
- The current power mode is visible from the tray: in the icon or beside it
  at a glance (decide in a mockup which reads at 16 to 24 px, and show the
  owner), and by name in the tray's menu and popup, with the modes the
  machine offers (`/sys/firmware/acpi/platform_profile_choices`). A machine
  without power modes shows nothing for it, not a guess.
- Changing the mode from the tray's menu, through the same daemon path the
  System page uses (ADR 0029), if the owner wants it there: ask with the
  mockup.
- It follows changes at once (the mode can change from the System page, from
  the automatic switch on charger change, or from another program).

## Done when

- Tests for each battery state's picture and each mode's display, a machine
  with no modes, and a mode changed by something else.
- Seen in the panel in a sandbox, light and dark, zh-TW and English, and on
  the owner's machine after the next release.

## Notes

**Blocked:** the owner: the power mode item's look is designed with the
owner in an icon design round; the owner decides whether the menu also
switches the mode; and the panel is seen on the owner's machine after a
release.

### Decisions (owner, 2026-11-22)

- The taskbar item draws **no battery**. The panel already has the
  desktop's own battery icon, and a second beside it is a duplicate. This
  replaces the first bullet of "What to build". The battery stays in the
  menu, the tooltip and the popup as words (charge, state, limit, time) from
  `battery_look`. The desktop's battery picture belongs to the top bar's
  status area in the Better OS session (ticket 157, which designs every
  state's picture with the owner); under GNOME it cannot be changed from
  outside the shell (spec).
- The pixel pictures the first branch drew were judged ugly and are gone,
  not kept for later. The power mode item's look gets its own design round
  with the owner.

### Built

- The readings icon offers no battery: `Reading` has no battery, "Show in
  the taskbar" no battery check box, and a choice stored before this that
  named the battery is rewritten without it when the item starts (the
  battery alone falls back to processor and memory, so the icon is never
  empty). The other bars have their widths from before ticket 295 back.
- The power mode is a second panel item (`monitor_tray::PowerModeItem`) with
  the icon theme's `power-profile-*-symbolic`, on a connection of its own
  that exists only while there is a mode to show, so every watcher forgets
  it when there is none (COSMIC's status area ignores `Passive`). It is
  registered and reported apart from the icon. The menu (shared), the
  tooltips and the popup name the mode and the System page's caution
  sentences through one rule, `hardware_core::power::mode_shown`;
  `Caution::words` and `Profile::name` are the System page's words, which it
  now calls too.
- The modes come from power-profiles-daemon (`hardware_platform::bus::
  follow_power_profiles`), only from one that is running: a stopped or
  masked daemon is absent and is never started through D-Bus activation, for
  the System page too. A watch that ends says "could not be read" last.
  Not from `/sys/firmware/acpi/platform_profile_choices` as this ticket's
  text says: ADR 0017 keeps Better OS off `platform_profile`. The System page
  sets the mode through the daemon directly, not through ADR 0029's path; a
  menu switch would call `bus::set_power_profile` the same way.
- A debug build of `better-monitor-service` reads `/sys` from
  `BETTER_OS_DEBUG_SYS_ROOT` like the window (`ServiceConfig::for_session`).

### Checked

- Tests in `monitor-tray` (`menu::tests`, `lib::tests`, `tests/power.rs`),
  `hardware-core` (`power::tests`), `hardware-platform` (`tests/bus.rs`:
  nothing activated on a bus that could start the daemon; a closed bus ends
  unreadable), `monitor-gui` (`popup::tests`), `manager-gui`.
- Screenshots in `/home/tim/Developer/better-os-evidence/295/`, headless
  GNOME Shell 46 with the Zorin AppIndicator extension, a stand-in daemon on
  a private system bus, a fixture `/sys` and `/proc` (processes `systemd`
  and `bash`): each mode with the menu open, English and zh-TW, dark and
  light (`mode-*`), the popup (`popup-*`), and no modes
  (`no-power-modes-zh-TW-light.png`: the item has left the panel). Never the
  machine's own power mode.
- Read from source only, not run: COSMIC's status area (ignores `Status`),
  dbus-daemon's and dbus-broker's activation errors. Not checked: swaybar,
  waybar, KDE, the owner's desktop.
