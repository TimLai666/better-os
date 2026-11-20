# Ticket 236 — Power and battery in Better Manager: screen and sleep

**Spec:** `design/system-settings-spec.md` section 10; ADR 0042 (proposed)
**Blocked by:** 231, 206, the owner's decision M4, and the owner's decision M1

## Why

When the screen turns off and when the PC sleeps are COSMIC Settings' Power
page in the session and GNOME Settings' under GNOME; Better Manager's power
cards sit on the System page today.

## What to build

- System › Power and battery: today's Power group moved unchanged, the Screen
  and sleep group (`com.system76.CosmicIdle` in the session; `idle-delay` and
  the `sleep-inactive-*` keys under GNOME), "set elsewhere" for a value off the
  list, the Keep awake link, other devices' batteries from UPower, and ticket
  170's holding-at-limit state in the battery card from the shared words.
- The bar's battery settings link (`design/top-bar-spec.md` 6.5) opens
  `--page system/power`.

## Done when

- [ ] Tests for each choice in both stores, a value written elsewhere, a
      sleep type that is not suspend under GNOME (shown as set elsewhere), no
      battery (the battery cards not drawn), UPower absent.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/236/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
