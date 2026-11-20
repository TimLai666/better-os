# Ticket 232 — Network and internet in Better Manager

**Spec:** `design/system-settings-spec.md` section 6; ADR 0042 (proposed)
**Blocked by:** 231, the owner's decision M4 (GNOME's airplane mode through `gsd-rfkill`), and the owner's decision M1

## Why

Wi-Fi, wired, VPN, the hotspot, and airplane mode are changed weekly. In the
session only COSMIC Settings offers them, and it has no hotspot (F2); under
GNOME, GNOME Settings. The bar's flyout (ticket 157) only switches.

## What to build

- `network-core` and `network-platform` (section 3.1), shared with the bar's
  network part (ticket 157) and its agent (ticket 217): devices, access points,
  active and saved connections, connectivity, airplane mode, the states and
  failure reasons of section 6.1, every write followed by a read.
- The pages of sections 6 to 6.3: the landing, Wi-Fi with inline personal
  keys, hidden networks, known networks and Forget, Properties with IP and DNS
  editing and metered, sharing by QR code, Ethernet, VPN with import through
  `nmcli connection import` and removal, the mobile hotspot, airplane mode
  (`/dev/rfkill` in the session, `gsd-rfkill` under GNOME), and the link card to
  `nm-connection-editor`.
- Privilege by section 3.3: own connections with `settings.modify.own`, a
  system connection with the administrator note; nothing through
  `manager-daemon`.
- The bar's network settings link opens `--page network` in both desktops
  (ticket 157 or 160, whichever is open, gets the one-line change).
- Kit: the password box with reveal and the QR code element (section 16).

## Done when

- [ ] `network-core` tests against a fake NetworkManager on a private bus:
      scan, connect with a key, wrong key, enterprise handed to the agent,
      forget, hidden network, manual IPv4, VPN import success and failure,
      hotspot on one radio, airplane mode on and off, NetworkManager absent,
      polkit refusal; every string in both languages.
- [ ] The bar and the page draw the same state from the same crate (a test
      greps for a second NetworkManager proxy).
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/232/`. The VM joins a WPA2 network a `hostapd` offers, imports a WireGuard file, and starts the hotspot.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
