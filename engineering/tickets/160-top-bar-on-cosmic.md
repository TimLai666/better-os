# Ticket 160 — The top bar on COSMIC

**Blocked by:** 143 and the owner's decision there to support COSMIC; 152

## Why

`design/top-bar-spec.md` section 11: on COSMIC, which offers layer-shell, the
bar needs no shell relay. If ticket 143 makes COSMIC a supported desktop, the
bar must work there too, or a person switching desktops loses it.

## What to build

- `top-bar-gui` opens its window as GPUI's `WindowKind::LayerShell`, anchored
  to the top edge with an exclusive zone of 32px, when the compositor offers
  layer-shell, and the service skips the relay there.
- For each part that reads GNOME Shell through the relay (focused window,
  input method, notifications, GNOME's panel), use the COSMIC source ticket
  143's evaluation names, or draw the part as unavailable with the spec's copy
  where COSMIC has none.
- Whether COSMIC's own panel is turned off while the bar runs, and how it is
  put back, as ticket 143's ADR decides.

## Done when

- [ ] In a COSMIC VM, the bar appears, reserves its strip, and every part
      either works or says it is unavailable; turning it off restores COSMIC's
      panel. Screenshots in `/home/tim/Developer/better-os-evidence/160/`.
- [ ] Unit tests for the source selection per desktop.
- [ ] fmt, clippy on stable, and the crates' tests pass.
