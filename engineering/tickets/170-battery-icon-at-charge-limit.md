# Ticket 170 — The battery icon shows "plugged in, holding at the limit"

**Blocked by:** none

**Blocked:** the owner seeing the held state in each Better OS place on this
machine, which needs the next release installed. Everything else is done.

## Why

The owner asked on 2026-11-21: when the charger is connected and the battery
has reached its charge limit, the battery icon must show that. Today it looks
the same as a laptop running on battery.

## What was seen (read only, 2026-11-21, the owner's machine)

- Charger connected (`/sys/class/power_supply/A*/online` is 1), charge limit
  80 (`charge_control_end_threshold`), battery at 80%.
- The kernel reports `Not charging`; UPower 1.90.3 reports the state
  `pending-charge`.
- GNOME Shell's battery indicator in Zorin's taskbar then draws the plain
  80% battery with no charging mark. Why: not yet read from the shell's source
  on this machine. From memory of GNOME Shell 46's `status/system.js`, the
  charging mark is added only for the `charging` state, so `pending-charge`
  draws as a discharging battery; confirm this from the installed gresource
  before building on it.

## What to build

- One rule, in one GPUI-free place, for what a battery icon shows: on battery,
  charging, plugged in and holding at the charge limit, plugged in and full,
  and unknown. "Holding at the limit" is its own state with its own icon (a
  plug or a shield mark, in the suite's icon style) and its own words
  (「已接上電源，維持在 80%」 / "Plugged in, holding at 80%"), never the
  on-battery icon. The state comes from the charger being online together with
  the battery not charging at or near the limit Better OS set, not from the
  percentage alone, and a missing reading is shown as unknown.
- Every battery icon Better OS draws uses that rule: Better Monitor's battery
  card and tray, Better Manager's System page (the charge limit card), Better
  Awake where it shows the battery, and the top bar's battery (ticket 157).
- The desktop's own battery icon, which is what the owner looks at today. It is
  drawn by GNOME Shell, so find out with evidence what can change it without
  shell code that decides anything (ADR 0012 and 0018's bounds): what the shell
  reads (UPower's state, its `icon-name`, the display device), whether UPower's
  own charge-threshold support (`charge-threshold-enabled`, in the UPower
  version each supported release ships) makes the shell draw the held state,
  and whether a newer GNOME already does. If only shell code can do it, write
  the ADR for the relay it would need and ask the owner; until the top bar
  (tickets 151 to 159) replaces that indicator, say on the System page's
  charge limit card what the desktop's icon will look like at the limit.

## Done when

- Tests for the rule in each state, including a charger that is online with a
  battery below the limit and not charging (a real fault, not "holding").
- The held state seen on the owner's machine in each Better OS place, zh-TW
  and English, light and dark.
- The desktop's own icon either shows the held state or the ticket records,
  with the evidence, why it cannot yet and which ticket will.

## Under ADR 0038's option D (2026-11-21)

In the Better OS session the desktop's battery icon is the top bar's
(ticket 157), which draws this ticket's rule, so the held state shows there
with nothing of GNOME's to change. The GNOME half of the last bullet above
shrinks to GNOME's own indicator while GNOME is the default: find out with
evidence what it draws at the limit and whether anything outside shell code
changes it, and if only shell code can, say so on the System page's charge
limit card; do not write the relay ADR now, because the GNOME bar (tickets
151 to 159) waits under option D.

## Notes

### Done (2026-11-21)

- The rule is `hardware_core::charge_state` (spec:
  `engineering/specs/battery-charge-state.md`). `hardware-core` because every
  place that draws a battery can depend on it without GPUI or I/O (it needs
  only `serde`), and the System page's battery and limit facts already live
  there. `battery_look` gives state, picture and words; its tests cover every
  state and the edges, including a charger online with the battery below the
  limit and not charging (`PluggedInNotCharging`, never called holding).
- New pictures in `better-ui`: `battery-plugged` (held) and
  `battery-full-plugged` (plugged in and full), a white plug over the battery
  like the charging bolt.
- Every battery icon and line uses it: Better Monitor's Overview card and
  list row, Battery page and taskbar popup, the taskbar item's menu line;
  Better Manager's Home banner, System battery and charge limit cards, Keep
  awake charge card; Better Awake's power row.
  `crates/hardware-core/tests/one_battery_rule.rs` fails when a window maps
  a status or a level to a picture or words of its own.
- The desktop's own icon under option D: GNOME Shell 46's `system.js`
  (extracted from the installed gresource) draws UPower's `pending-charge` as
  the plain level icon, UPower 1.90.3 has no charge-threshold property, and
  nothing outside the shell's code decides the name (the spec's "desktop's
  own battery icon" section, with GNOME 42, 47, 48 and 49 compared). The
  charge limit card says so on GNOME while the limit is under 100%. No relay
  ADR, as option D says.
- Screens of every place in the held state, zh-TW and English, light and
  dark, from fixture readings in a sandbox (headless GNOME Shell, private
  buses with only dconf and a11y activatable, the hardware and Awake
  stand-ins): `/home/tim/Developer/better-os-evidence/170/`. The Keep awake
  page was shot in light only (its dark run came out light, not chased); the
  System page is in all four. `gnome-own-indicator-at-limit-zoom.png` and
  `gnome-own-indicator-evidence.txt` are the sandbox shell's own indicator
  drawing the real machine at its limit: a plain battery, no mark.

### Left

- The held state seen by the owner in each place above, zh-TW and English,
  light and dark, after the next release is installed. What they should see
  at 80% with the limit at 80 and the charger in: the plug picture and
  「已接上電源，維持在 80%」 / "Plugged in, holding at 80%" on Monitor's
  battery card, Battery page, popup and taskbar menu; on Manager's Home
  banner, System battery card, charge limit card (which also says what
  GNOME's own icon shows) and Keep awake charge card; and on Awake's power
  row. GNOME's own indicator keeps drawing a plain battery until the top bar
  (ticket 157) draws the desktop's battery.
- Not seen anywhere yet: the taskbar item's menu (no StatusNotifier host in
  the sandbox; its words are tested in `monitor-tray`), Home's banner (it
  reads `/`, not the debug fixture root; tested in `home.rs`), and how far this
  machine's firmware lets the charge fall before charging again (the
  five-point window is a starting value).
