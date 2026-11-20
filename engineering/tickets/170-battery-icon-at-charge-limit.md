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

### Done (2026-11-21, revised after review the same day)

- The rule is `hardware_core::charge_state` (spec:
  `engineering/specs/battery-charge-state.md`). `hardware-core` because every
  place that draws a battery can depend on it without GPUI or I/O (it needs
  only `serde`), and the System page's battery and limit facts already live
  there. `battery_look` gives the state, its picture, and its words in full
  and short. States: on battery; plugged in and still draining; charging;
  held at the limit (naming the charge under the limit, and "not charging
  above the limit" over it); plugged in and full; plugged in and not charging
  below the limit (never called holding); unknown.
- The rule's edges, each tested in `charge_state.rs`: a start threshold opens
  the hold window only when it is above 0 and within 20 points of the limit
  (a ThinkPad's factory 0 does not); two batteries are one state with an
  energy-weighted charge (also tested through `hardware-platform`'s
  `thinkpad` fixture and Better Monitor's readings); an older kernel's
  `Unknown` is held inside a known limit's window and unknown elsewhere
  (before Linux 5.19, commit 185d20694a8a; Ubuntu 22.04's GA 5.15 can show
  it); no two states share a picture at the same charge.
- Which power supplies count as a charger (Mains, USB, Wireless; not a UPS,
  from the kernel's `sysfs-class-power` `type`) and whether the machine is on
  battery are decided only by the rule: Better Awake's service, the System
  page, Better Monitor's window and tray ask it.
  `crates/hardware-core/tests/one_battery_rule.rs` fails when a window maps a
  status, a level, a power supply's type or a charger's state to a picture or
  words of its own, or reads "on battery" beside the rule.
- Pictures in `better-ui`: `battery-plugged` (held), `battery-full-plugged`
  (full), `battery-plugged-caution` (not charging below the limit, or still
  draining): a dark plug drawn on the battery where the charging picture has
  its bolt, over a green charge, a full green charge, or a short amber charge
  with an amber caution mark. Drawn three times on 2026-11-21 until the plug
  read at 16 and 20 px on both window backgrounds (the sheet below).
- Every battery icon and line uses it: Better Monitor's Overview card (short
  words in the header, the time or the energy at the foot) and Performance
  list row (short words), Battery page (no time row unless the charge
  moves), taskbar popup, taskbar menu line; Better Manager's Home banner,
  System battery and charge limit cards, Keep awake charge card; Better
  Awake's power row, read off the UI thread and followed through the
  service's signals and UPower.
- The desktop's own icon under option D: GNOME Shell 46's `system.js`
  (extracted from the installed gresource) draws UPower's `pending-charge` as
  the plain level icon, UPower 1.90.3 has no charge-threshold property, and
  nothing outside the shell's code decides the name (the spec's "desktop's
  own battery icon" section, with GNOME 42, 47, 48 and 49 compared). The
  charge limit card says so on GNOME while a limit under 100% is set and the
  shell is older than 49 or UPower has no `ChargeThresholdEnabled`. No relay
  ADR, as option D says.

### Evidence (`/home/tim/Developer/better-os-evidence/170/`)

Taken in a sandbox: a headless GNOME Shell 46 in a 4G scope, its own home
and dconf, a session bus whose only activatable services are dconf, the
accessibility bus and the desktop portals (so the windows follow the
sandbox's light or dark setting), a private system bus with the hardware
stand-ins, and Better Awake's stand-in (the real engine with a fake
inhibitor); `busctl --user list` showed no installed Better OS service on it.

- From the `held` fixture (charger online, `Not charging`, 80% of an 80%
  limit), zh-TW and English, light and dark: `manager-system-*`,
  `manager-keep-awake-*`, `monitor-overview-*`, `monitor-battery-page-*`,
  `monitor-popup-*`, `awake-battery-*`.
- From this machine's own `/sys`, read only, while it was held at 80 of 80:
  `manager-home-real-sys-*` (Home reads `/`, not the fixture) and
  `monitor-tray-menu-en-real-sys.png` / `-zh-TW-real-sys.png` (the debug
  tray and service, shown by the sandbox shell's own StatusNotifier host;
  the menu is GNOME's, so it has no light variant of its own).
- `battery-pictures-sheet-light-dark.png`: every battery picture at the
  gallery's sizes on `#f3f3f3` and `#202020`, from the real assets.
- `gnome-own-indicator-at-limit-zoom.png` and
  `gnome-own-indicator-evidence.txt`: GNOME's own indicator drawing this
  machine at its limit, a plain battery with no mark.

### Left

- The held state seen by the owner in each place, zh-TW and English, light
  and dark, after the next release is installed. What they should see at 80%
  with the limit at 80 and the charger in: the battery with a dark plug on
  its green charge and 「已接上電源，維持在 80%」 / "Plugged in, holding at
  80%" (in short
  「維持在上限 80%」 / "Held at 80% limit") on Monitor's battery card, Battery
  page, popup and taskbar menu; on Manager's Home banner, System battery card,
  charge limit card (which also says GNOME's own icon looks the same as on
  battery) and Keep awake charge card; and on Awake's power row. GNOME's own
  indicator keeps drawing a plain battery until the top bar (ticket 157)
  draws the desktop's battery.
- How far this machine's firmware lets the charge fall before charging again
  has not been watched (the five-point window is a starting value).

### A question for the owner

Better Monitor's taskbar icon draws the battery as a level bar or a number,
the same colour in every state, so held, on battery and charging look the
same in the panel; only its menu line says the state. Two readings of "Better
Monitor's … tray" in this ticket: (a) the tray's words, which now follow the
rule, are what was meant, and the panel icon is a meter like the processor's;
or (b) the panel icon itself should show the state. (b) would take a small
mark drawn in the icon's own pixels for each state the rule has (a plug, a
bolt, a caution mark) at 22 px beside the battery's bar or number, in the
icon's single-colour panel style (DESIGN.md: a tray icon stays symbolic), and
tests of the pixels as the icon's other tests do. Which one?
