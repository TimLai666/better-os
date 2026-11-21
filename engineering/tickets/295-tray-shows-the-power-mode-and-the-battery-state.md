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

**Blocked:** the owner decides whether the menu also switches the power mode
(the mock-up `better-os-evidence/295/mockup-power-mode.png` shows both
menus), and the panel is checked on the owner's machine after the next
release; everything else is built.

### Built (2026-11-22)

- The battery's picture in the icon is the battery rule's `BatteryIcon`
  drawn in pixels (`monitor_tray::icon::battery_drawing`): a battery with a
  cap, filled to the charge (amber when low), and a bolt, a plug, an amber
  "!" or a "?" over it; in the numbers style the same mark, or a small
  battery for a level, beside the charge. Spec: "The taskbar item" in
  `engineering/specs/battery-charge-state.md`. The first sandbox look showed
  a 5×7 mark on a 9-pixel battery is a blur once GNOME draws the icon at
  16 px, so the battery takes a 14-pixel slot with the mark at double size
  when it is shown with one other reading or alone.
- The power mode is a second panel item beside the icon
  (`monitor_tray::PowerModeItem`, `/PowerModeItem` on the tray's own
  connection), drawn from the icon theme's `power-profile-*-symbolic`, so it
  takes the panel's colour light and dark. The mock-up compared it with a
  mark inside the readings icon, which at 16 px is 5 pixels over the memory
  bar; the themed item reads at every size. It is `Passive` (hidden) on a
  machine without modes, when the daemon's active mode is not one it lists,
  or when the person unticks "Power mode" under "Show in the taskbar".
- The menu (shared by both items) and the tooltip say "Power mode: …" /
  「電源模式：…」 in the System page's words, now `Profile::name` in
  `hardware-core` (the System page's copy calls it too); the popup has a
  Power mode card listing every mode the machine offers with the one in
  force marked.
- The modes come from power-profiles-daemon, read by
  `hardware_platform::bus::follow_power_profiles` (new; the watch is in place
  before the first read), not from `/sys/firmware/acpi/platform_profile_choices`
  as this ticket's text says: ADR 0017 keeps Better OS off `platform_profile`,
  which the daemon drives, and on such a machine the daemon's list comes from
  it. Likewise the System page sets the mode through the daemon directly, not
  through `manager-daemon` (the ticket names ADR 0029, which is about root
  writes); a menu switch, if the owner wants one, would call
  `hardware_platform::bus::set_power_profile` the same way.
- A debug build of `better-monitor-service` now reads `/sys` from
  `BETTER_OS_DEBUG_SYS_ROOT` like the window (`ServiceConfig::for_session`,
  one place for both), so a sandbox can show every battery state in the
  taskbar.

### Checked

- Tests: each state's picture and that no two of the six states look the
  same in either style (`monitor-tray` `icon::tests`), each mode's line,
  tooltip and themed icon, no modes, an unknown or unreadable mode
  (`menu::tests`), a mode changed by another program reaching the item and
  the menu without a layout change (`lib::tests`, and `tests/power.rs`
  against a stand-in daemon on a private bus, including a daemon that starts
  late and one with no modes), the popup's block (`monitor-gui`
  `popup::tests`).
- Screenshots in `/home/tim/Developer/better-os-evidence/295/`: every battery
  state and every mode with the menu open, English and zh-TW, dark and light
  (`battery-*-{en,zh-TW}-{dark,light}.png`, `mode-*-….png`), the icons alone
  at panel size zoomed 4× in both styles and both panels
  (`panel-icons-{bars,numbers}-{dark,light}.png`), the popup
  (`popup-{en,zh-TW}-{dark,light}.png`), and a machine with no modes
  (`no-power-modes-en-light.png`: the item is gone and the menu has no mode
  line). Headless GNOME Shell 46 with the Zorin AppIndicator extension (the
  light panel is GNOME's `light-style` extension), a stand-in
  power-profiles-daemon on a private system bus whose mode was changed by
  `gdbus` as another program would, a fixture `/sys`; never the machine's
  own power mode or limit. The scripts are in `scripts/` beside them. The
  processor, memory and network figures in the menus are the sandbox
  machine's real ones; the battery is the fixture's.
- Not checked: swaybar or waybar hiding a `Passive` item, and the owner's
  desktop.
