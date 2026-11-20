# Ticket 158 — The top bar's clock, calendar, and notifications

**Blocked by:** 152, 161 (its design of a notification island and a
right-edge notification center may replace this flyout's notification list)

## Under ADR 0038's option D (2026-11-21)

ADR 0038's table: "build after 161, notifications from Better OS's own
daemon". In the Better OS session nothing of GNOME's owns
`org.freedesktop.Notifications`; Better OS's notification service (ticket 215)
does, and the list, Do not disturb, activation, dismissal, and Clear all come
from it rather than from the relay. If ticket 161's N2 makes the right-edge
center the one list, this flyout keeps the calendar only and the bell opens
the center (ticket 216). The clock's settings in the session are ticket 160's
source. The GNOME half (the relay's notification members, GNOME's
`show-banners`, the banner check in the headless shell) waits with ticket
151.

## Why

`design/top-bar-spec.md` sections 6.6 and 6.7: the clock follows GNOME's clock
settings, and the clock and bell open one flyout with a calendar and GNOME's
notifications.

## What to build

- **Clock** (6.6): `clock-format`, `clock-show-weekday`, `clock-show-date`,
  `clock-show-seconds` read live; ticket 146's local-time function; the
  narrow form below 1280px; the no-time-zone-data case; redraw only on the
  minute or second boundary.
- **Flyout** (6.7), shape (a), which the owner chose on 2026-11-21 (D7): long date, month calendar with the locale's
  first weekday, the notification list from the relay's notification members
  (app, title, body with Show more, time, actions, close), Clear all, Do not
  disturb through `org.gnome.desktop.notifications show-banners`, empty and
  unavailable states, the bell's dot and crossed state. The list is kept in
  memory only.
- Check, in the headless shell, that GNOME's notification banners are not drawn
  over the bar (spec section 14); if they are, move them below the strip
  through the relay as an action the shell performs, and record how.

## Done when

- [ ] Unit tests: every clock format in both languages, calendar layout for a
      month starting on each weekday, the notification list model with adds,
      removals, actions, and Clear all.
- [ ] In a headless GNOME Shell 46 (sandbox rules): `notify-send` and a
      `GNotification` test both appear in the flyout, activating and
      dismissing work in the shell, Do not disturb stops GNOME's banners.
      Screenshots in `/home/tim/Developer/better-os-evidence/158/`, compared
      with `design/top-bar/clock-flyout-*.html`.
- [ ] fmt, clippy on stable, and the crates' tests pass.
