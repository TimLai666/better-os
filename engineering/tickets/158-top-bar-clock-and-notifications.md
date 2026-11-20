# Ticket 158 — The top bar's clock, calendar, and notifications

**Blocked by:** 152, and the owner's decision D7 in `design/top-bar-spec.md`

## Why

`design/top-bar-spec.md` sections 6.6 and 6.7: the clock follows GNOME's clock
settings, and the clock and bell open one flyout with a calendar and GNOME's
notifications.

## What to build

- **Clock** (6.6): `clock-format`, `clock-show-weekday`, `clock-show-date`,
  `clock-show-seconds` read live; ticket 146's local-time function; the
  narrow form below 1280px; the no-time-zone-data case; redraw only on the
  minute or second boundary.
- **Flyout** (6.7), shape (a): long date, month calendar with the locale's
  first weekday, the notification list from the relay's notification members
  (app, title, body with Show more, time, actions, close), Clear all, Do not
  disturb through `org.gnome.desktop.notifications show-banners`, empty and
  unavailable states, the bell's dot and crossed state. The list is kept in
  memory only.
- If the owner chooses (b): the clock calls the relay to open GNOME's own
  date menu instead, and the notification members are not used; the relay
  members stay unused rather than removed until the owner decides otherwise.
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
