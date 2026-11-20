# Ticket 215 — Notifications arrive as Better OS's island in the session

**Spec:** better-os-session.md S4 (notifications row)
**Blocked by:** 200, 203, 161 (its design and the owner's N1, N2, N4)

## Why

`cosmic-notifications` is not installed, so in the session nothing owns
`org.freedesktop.Notifications` until Better OS does: a notification sent by
any application would vanish. Ticket 161 designs the island and the center;
in the session both are Better OS's own layer surfaces with no shell code.

## What to build

- **Crate `notification-service`**: owns `org.freedesktop.Notifications`
  in the session (`Notify`, `CloseNotification`, `GetCapabilities` with
  `actions`, `body`, `body-markup` limited to the specification's tags,
  `icon-static`, `persistence`; `GetServerInformation`; the
  `NotificationClosed` and `ActionInvoked` signals, `ActivationToken`), also
  the portal's notifications through `xdg-desktop-portal`'s own backend
  choice, the list kept in memory only (top bar spec 6.7), Do not disturb, and
  an interface for the island, the center (216), and the top bar's bell (158)
  checked by executable.
- **The island** as ticket 161's design: a layer surface on the top layer,
  top right under the bar, the stacking, timeouts, urgency, Do not disturb,
  fullscreen, several monitors, pointer and keyboard, screen reader
  announcement, reduced motion, every state in both languages.
- A user unit `WantedBy=better-os-session.target`; under GNOME it does not
  start (GNOME Shell owns the name there).

## Done when

- [ ] Unit tests for the specification's rules (replaces-id, expiry,
      urgency, actions, markup sanitizing, a sender that leaves the bus), the
      island's model in both languages.
- [ ] A private-bus test: `notify-send`, a `GNotification` through the portal,
      a Firefox notification in the VM, each shown, acted on, and closed with
      the right signals.
- [ ] Screenshots light and dark in
      `/home/tim/Developer/better-os-evidence/215/`, compared with ticket 161's
      mockups.
- [ ] fmt, clippy on stable, the crates' tests, and `verify-deb.sh` pass.
