# Ticket 247 — The lid and the power button

**Spec:** `design/system-settings-spec.md` section 13 (Lid and power button); ADR 0042 (proposed)
**Blocked by:** 236, the owner's decision M9, ADR 0042 accepted, and the owner's decision M1

## Why

Neither COSMIC Settings nor GNOME Settings 46 offers what closing the lid
does. logind decides it from `/etc/systemd/logind.conf`, which only root can
write.

## What to build

- First check, in the VM, whether logind 255 rereads a drop-in without a
  restart; if a restart of `systemd-logind` is needed, stop and ask (it ends no
  session, but it is a privileged restart).
- `manager-daemon`: `SetLidAction(on_battery: s, plugged_in: s)` and
  `SetPowerButtonAction(s)` on `org.betteros.Hardware1`, closed values, one
  polkit action each, one drop-in `/etc/systemd/logind.conf.d/60-better-os.conf`
  written, read back, and removed with Better Manager.
- The cards in Power and battery; under GNOME the power button is
  `power-button-action`.

## Done when

- [ ] Daemon tests for each value and a refused one; the drop-in removed on
      uninstall; container check of the file written.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/247/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
