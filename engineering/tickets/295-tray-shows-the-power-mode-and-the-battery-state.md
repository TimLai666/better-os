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
