# Ticket 254 — Background, GlobalShortcuts, Wallpaper and InputCapture portals in the Better OS session

**Spec:** better-os-session.md S4 (the "Portals COSMIC lacks" row)
**Blocked by:** 203

## Why

Ticket 201 asked `xdg-desktop-portal` in the session which interfaces it
offers (`*/out/inv1/portals.txt`): `org.freedesktop.portal.Background`,
`GlobalShortcuts`, `Wallpaper`, and `InputCapture` do not exist there,
because only GNOME's backend implements them and it is `UseIn=gnome`. A
Flatpak application that asks to run in the background or to autostart, or
one that registers a global shortcut (chat and recording applications do),
gets "no such interface" in the session and works under GNOME.

## What to build

- **Find out, per interface, who can serve it in the session**: whether
  `xdg-desktop-portal-gnome`'s backend runs outside GNOME Shell for the
  interface (Background needs a window list, which it takes from GNOME
  Shell's introspection and which does not exist here); whether a newer
  `xdg-desktop-portal-cosmic` release serves it; or whether Better OS serves
  it (Background's permission dialog and autostart file are small; a
  GlobalShortcuts backend needs the compositor's key grab, which only
  `cosmic-comp`'s shortcuts can give).
- For each interface: a provider listed in `betteros-portals.conf` (ticket
  203's package), or a row in the spec saying it is not offered and what an
  application sees.

## Done when

- [ ] The spec's portal rows name a provider or a reason for each of the four.
- [ ] In ticket 201's VM, a Flatpak application's background request and a
      global-shortcut registration behave as the spec says; evidence in
      `/home/tim/Developer/better-os-evidence/254/`.
