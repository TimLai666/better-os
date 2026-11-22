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
  logical pixels tall: the theme's `power-profile-*-symbolic`, found by the
  shared icon theme lookup (`better_core::icon_theme`, moved from
  `better-ui`, which re-exports it), parsed and written out again by `usvg`
  so it is self-contained, recoloured to the panel's ink; a gap; the
  readings drawn natively on the 16-pixel grid. The ink follows the shell
  theme, then the GTK theme's `-Light`/`-Dark` name, then the colour scheme;
  when none says, the mark has the readings' light ink and dark edge. Spec:
  "The power mode in the same item".
- The panel gets it as a file named by `IconName` (GNOME's AppIndicator
  extension squeezes a wide pixmap into a square), written under two names
  in turn so GNOME Shell keeps two file monitors, not one per picture, and
  not written at all when the picture did not change. The pixmaps are left
  out only for GNOME Shell's watcher; other panels keep them. The files go
  when the item ends (checked with SIGTERM in the sandbox) or starts.
- A mode change is a new picture on the same item: no second item, no
  second connection, no new registration.
- The menu, the tooltip and the popup name the mode and the System page's
  caution sentences through one rule, `hardware_core::power::mode_shown`.
- The modes come from power-profiles-daemon; every call carries
  `NO_AUTO_START` (`hardware_platform::bus::invoke`), so no read or write
  starts a stopped or masked daemon, for the System page too. A watch that
  ends says "could not be read" last. Not from
  `/sys/firmware/acpi/platform_profile_choices` as this ticket's text says:
  ADR 0017 keeps Better OS off `platform_profile`. The System page sets the
  mode through the daemon directly, not through ADR 0029's path; a menu
  switch would call `bus::set_power_profile` the same way.
- A debug build of `better-monitor-service` reads `/sys`, and `/proc` and
  `etc/passwd` when the fixture has them, from `BETTER_OS_DEBUG_SYS_ROOT`
  (`ServiceConfig::for_session`).

### Measured

- 10,000 picture changes in the sandbox's GNOME Shell 46 (`scripts/churn.py`,
  `scripts/leak.sh`, one item changing every 20 ms): under a new name each
  time (the first design) resident memory went from 274,244 kB to
  277,712 kB (+3.4 MB); under two names in turn from 270,828 kB to
  273,692 kB (+2.9 MB). Open files went from 61 to 52 and 60 to 52, inotify
  watches from 11 to 12 in both: GLib watches the directory, so the per-path
  monitors the source shows (`st-texture-cache.c` 1173-1191) cost memory and
  main-loop sources, not watches. Two names caps them at two.
- One minute of this machine's live readings at scale 2, before identical
  pictures were skipped: 34 writes; the tray 0.44 s of processor, GNOME
  Shell 0.61 s, against 0.45 s and 0.37 s with no mark.

### Limits found

- The extension's own `icon-size` setting (20, 24) does not grow a picture
  drawn at its own size; the item stays 16 pixels tall. Following it would
  mean reading that extension's private settings.
- At 150 % on ZorinBlue-Dark the mark shows a dark rim
  (`mode-performance-zorin-dark-en-scale150-panel-zoom.png`) although the
  file says white ink with no edge; where the shell adds it was not found.
- Right after login the panel's colour is read once the settings portal
  answers; until then the mark has the light-with-dark-edge look, which
  reads on both.
- KDE Plasma would draw the file through Qt's SVG renderer, which has no
  filters, so the mark would keep the theme's colour there (read from
  source, not run).

### Checked

- Tests: `monitor-tray` (`picture`: the readings the same with and without
  the mark and every cell kept at 100, 125, 150 and 200 %, the ink per
  panel, the panel rule, icons with Inkscape namespaces, an entity, a
  comment holding `<svg` and a style composed and drawn by librsvg through
  `gdk-pixbuf-thumbnailer`; `picture_file`: two names, no rewrite of the
  same picture, nothing left after closing; `icon_theme`; `menu`; `lib`;
  `tests/power.rs`), `better-core` and `better-ui` (the shared lookup),
  `hardware-core`, `hardware-platform` (`tests/bus.rs`: nothing activated
  on a bus that could start the daemon, before and after a daemon left; a
  closed bus ends unreadable), `monitor-gui`, `monitor-service`,
  `manager-gui` and the crates that depend on these.
- Screenshots in `/home/tim/Developer/better-os-evidence/295/`, headless
  GNOME Shell 46 with the Zorin AppIndicator extension, the User Themes
  extension applying ZorinBlue-Light (with `color-scheme` left at
  `default`, as Zorin leaves it) or ZorinBlue-Dark, a stand-in daemon on a
  private system bus, fixture `/sys` and `/proc`: each mode with the menu
  open (scale 1) and the panel at real size and zoomed (scale 1 and 2),
  English and zh-TW (`mode-*-zorin-{light,dark}-*`); the numbers style
  (`numbers-*`); a panel whose colour cannot be told
  (`panel-unknown-adwaita-default-*`); 125 % and 150 % (`*-scale125-*`,
  `*-scale150-*`, ZorinBlue-Dark); the popup; no modes; the wide
  pixmap against the wide file; a PNG against an SVG at scale 2. Never the
  machine's own power mode.
- Read from source only: Ubuntu's AppIndicator extension (`fbc758a4`),
  GNOME Shell 46.0's `st-texture-cache.c`, waybar (`b4f03e63`), KDE Plasma's
  system tray and KIconThemes, swaybar, COSMIC's status area (`cb3ea91a`),
  dbus-daemon's and dbus-broker's activation errors. Not run: KDE, waybar,
  swaybar, COSMIC, the owner's desktop.
