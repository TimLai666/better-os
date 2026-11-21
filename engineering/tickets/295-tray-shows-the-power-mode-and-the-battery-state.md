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
- The picture (`monitor_tray::picture`, GPUI-free) is an SVG document 16
  logical pixels tall: the theme's `power-profile-*-symbolic` found by the
  Icon Theme Specification's lookup and nested as it is, recoloured to the
  panel's foreground (white on dark, `#2e3436` on light, from the portal's
  `color-scheme`), a gap, then the readings' canvas as crisp squares. The
  icon theme and the colour scheme are followed through the settings portal
  (`icon_theme`). No mark, no file: the item is its square pixmaps.
- The panel gets it as a file: GNOME's AppIndicator extension fits a pixmap
  into a square but draws a wide image named by `IconName` at its own size,
  and an SVG at the display's scale (spec, file and line; seen at scale 2 in
  `scale2-png-vs-svg.png`, where a PNG was soft). The file is written under
  `$XDG_RUNTIME_DIR/better-os/monitor-tray/` with a new name per change
  (`picture_file`). The pixmaps are left out only for GNOME Shell's watcher,
  which would draw them over the file; waybar, KDE, swaybar and COSMIC keep
  them and fall back to the readings (read from source).
- A mode change is a new picture on the same item: no second item, no
  second connection, no new registration.
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

- The file follows the readings: 34 writes in a minute of live readings
  (sandbox, scale 2, tmpfs); in that minute the tray used 0.44 s of
  processor and GNOME Shell 0.61 s, against 0.45 s and 0.37 s without the
  mark. The extension has no second layer to keep the readings as pixmaps
  beside a file (its overlay is a small emblem).
- The extension's own `icon-size` setting (20, 24) does not grow a picture
  drawn at its own size; the item stays 16 pixels tall
  (`icon-size-24-*`). Following it would mean reading that extension's
  private settings.
- Light or dark comes from the portal's `color-scheme`; a light shell theme
  chosen without that preference (some Zorin layouts) is drawn as dark.

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
  panel at real size and zoomed, English and zh-TW, dark and light, at
  scale 1 and 2 (`mode-*-scale1-*`, `mode-*-scale2-*`); 125 % and 150 % with
  Mutter's `scale-monitor-framebuffer` (`*-scale125-*`, `*-scale150-*`);
  the extension at icon size 20 and 24; the popup; no modes; the wide
  pixmap against the wide file, and a PNG against an SVG at scale 2. Never
  the machine's own power mode.
- Read from source only: Ubuntu's AppIndicator extension (`fbc758a4`, the
  same rule as Zorin's copy), waybar (`b4f03e63`), KDE Plasma's system
  tray, swaybar, COSMIC's status area (`cb3ea91a`), dbus-daemon's and
  dbus-broker's activation errors. Not run: KDE, waybar, swaybar, COSMIC,
  the owner's desktop.
