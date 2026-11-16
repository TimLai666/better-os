# Ticket 139 — Sometimes the login screen never appears after a (re)boot

**Blocked by:** the next stuck boot with GDM debug logging on

## Why

The owner reported on 2026-11-16 that the machine often stays on the boot
splash, especially after Restart, once for good (forced power-off).

## What was seen (read only, all 34 recorded boots since 2026-07-30)

- Every normal boot: `gdm.service` starts at about 6 s and opens the greeter
  session (`New session c1 of user gdm`, `gdm-wayland-session`) about 4 s
  later.
- Boots 2026-11-16 20:42 (31 s, then reset) and 20:43 (16 min, then forced
  off): `gdm.service` started, and no greeter session was ever opened —
  nothing from `gdm-launch-environment` or `gdm-wayland-session` at all.
  `gnome-remote-desktop` timed out asking GDM over D-Bus at 31 s.
- Better OS units (`better-touchpad-scroll@event5`, `better-hardware-restore`)
  ran the same way in the stuck boots and in the good one right after them.
- A hang before the journal is flushed leaves no boot record, so earlier
  stuck boots the owner remembers may simply not be recorded.
- `ucsi_acpi ... GET_CONNECTOR_STATUS failed (-110)` appears in the 20:43
  boot (ticket 101); a link is not established.

## Next step

- GDM debug logging was turned on by the owner on 2026-11-16
  (`/etc/gdm3/custom.conf`, `[debug] Enable=true`). On the next stuck boot,
  read what GDM was waiting for (seat0 `CanGraphical`, the DRM device, logind)
  and fix at the cause; if a Better OS unit is involved, fix it there.
- Ask the owner whether Ctrl+Alt+F3 reaches a text login when stuck.
- Turn GDM debug logging back off once the cause is known.
