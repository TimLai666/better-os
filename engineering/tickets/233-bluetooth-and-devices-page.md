# Ticket 233 — Bluetooth and devices in Better Manager

**Spec:** `design/system-settings-spec.md` section 7; ADR 0042 (proposed)
**Blocked by:** 231, and the owner's decision M1

## Why

In the session the only way to pair a device is COSMIC Settings' Bluetooth
page: the bar has no Bluetooth item (`design/top-bar-spec.md` section 6) and
ticket 217's agent only answers requests. Under GNOME it is GNOME Settings.

## What to build

- `bluetooth-core` and `bluetooth-platform` (section 3.1): adapters,
  devices, battery, the pairing state machine and the `org.bluez.Agent1`
  requests, shared with ticket 217's bar agent.
- The page of section 7 with the Add device dialog: discoverable while the
  page is open, the page's own agent registered while it is open and the
  default agent only while the dialog is open, connect, disconnect, rename,
  remove, the sub-page cards, airplane-mode and no-adapter states.
- The question for the owner, written in this ticket's Notes when it starts:
  whether the bar gains a Bluetooth item (`design/top-bar-spec.md` has none).

## Done when

- [ ] `bluetooth-core` tests against a fake BlueZ on a private bus: every
      agent request answered and cancelled, pairing failure, a device leaving
      mid-pairing, a second request while one is open, the page closing during
      pairing (the agent is given back), adapter absent, rfkill-blocked.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/233/`. Pairing a real device is seen on the owner's machine in ticket 227.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
