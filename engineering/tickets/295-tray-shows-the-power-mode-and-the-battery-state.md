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

**Blocked:** the owner: the look of the mark and the picture is designed
with the owner in an icon design round; the owner decides whether the menu
also switches the mode; and the panel is seen on the owner's machine after
a release.

### Decisions (owner, 2026-11-22)

- The taskbar item draws **no battery**. The panel already has the
  desktop's own battery icon, and a second beside it is a duplicate. This
  replaces the first bullet of "What to build". The battery stays in the
  menu, the tooltip and the popup as words (charge, state, limit, time) from
  `battery_look`. The desktop's battery picture belongs to the top bar's
  status area in the Better OS session (ticket 157, which designs every
  state's picture with the owner); under GNOME it cannot be changed from
  outside the shell (spec).
- Better Monitor is **one button** in the panel, as a macOS menu bar item
  is: the power mode's mark and the readings in one picture, one menu, one
  tooltip. This replaces the second panel item built before it.
- The mark is the desktop's own power mode icon, never a drawing of ours.

### Built

- The readings icon offers no battery: `Reading` has no battery, "Show in
  the taskbar" no battery check box, and a choice stored before that named
  the battery is rewritten without it when the item starts (the battery
  alone falls back to processor and memory). The readings have their widths
  from before ticket 295.
- The picture (`monitor_tray::picture`, GPUI-free): the theme's
  `power-profile-*-symbolic` found by the Icon Theme Specification's lookup
  (`picture::find_icon`), drawn with `resvg` (`resvg 0.46`, GPUI's own),
  tinted by the readings' rule, then a gap, then the readings scaled to 16
  pixels. The theme comes from the settings portal and is followed
  (`icon_theme`). No mark, no icon: the picture is the square pixmaps.
- GNOME's AppIndicator extension fits a pixmap into a square and draws a
  wide PNG named by `IconName` at its own size (spec, with file and line), so
  with a mark the item writes the picture under
  `$XDG_RUNTIME_DIR/better-os/monitor-tray/` (`picture_file`) and sends no
  pixmaps. A mode change is a new picture on the same item: no second item,
  no second connection, no new registration.
- The menu, the tooltip and the popup name the mode and the System page's
  caution sentences through one rule, `hardware_core::power::mode_shown`.
- The modes come from power-profiles-daemon; every call carries
  `NO_AUTO_START` (`hardware_platform::bus::invoke`), so no read or write
  starts a stopped, masked or just-stopped daemon, for the System page too.
  A watch that ends says "could not be read" last. Not from
  `/sys/firmware/acpi/platform_profile_choices` as this ticket's text says:
  ADR 0017 keeps Better OS off `platform_profile`. The System page sets the
  mode through the daemon directly, not through ADR 0029's path; a menu
  switch would call `bus::set_power_profile` the same way.
- A debug build of `better-monitor-service` reads `/sys`, and `/proc` and
  `etc/passwd` when the fixture has them, from `BETTER_OS_DEBUG_SYS_ROOT`
  (`ServiceConfig::for_session`).

### Limits found

- The wide picture is drawn one file pixel to one logical pixel: soft on a
  scale-2 display, and the extension's "icon size" setting does not grow it
  (`icon-size-24-balanced-zh-TW-light-panel-zoom6.png`). Without a mark the
  item is its pixmaps and follows both, as before.
- A panel that does not take a path in `IconName` shows nothing while there
  is a mark. COSMIC's status area does take one (read from source); KDE,
  waybar and swaybar were not read or run.

### Checked

- Tests in `monitor-tray` (`picture`, `picture_file`, `icon_theme`,
  `menu`, `lib`, `tests/power.rs`), `hardware-core` (`power`),
  `hardware-platform` (`tests/bus.rs`: nothing activated on a bus that could
  start the daemon, including a daemon that leaves between calls; a closed
  bus ends unreadable), `monitor-gui` (`popup`), `monitor-service`,
  `manager-gui`, `awake-gui`, `window-attention-service`, `manager-daemon`.
- Screenshots in `/home/tim/Developer/better-os-evidence/295/`, headless
  GNOME Shell 46 with the Zorin AppIndicator extension and the Adwaita icon
  theme, a stand-in daemon on a private system bus, fixture `/sys` and
  `/proc` with invented round figures: each mode with the menu open and the
  panel at real size and zoomed 6×, English and zh-TW, dark and light
  (`mode-*`); the extension at icon size 24; the popup (`popup-*`); no
  modes; the wide pixmap against the wide file (`wide-pixmap-vs-file*`).
  Never the machine's own power mode.
- Read from source only: Ubuntu's AppIndicator extension (`fbc758a4`, the
  same rule as Zorin's copy), COSMIC's status area, dbus-daemon's and
  dbus-broker's activation errors. Not checked: KDE, waybar, swaybar, a
  scale-2 display, the owner's desktop.
