# Ticket 170 — The battery icon shows "plugged in, holding at the limit"

**Blocked by:** none

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
