# Ticket 251 — Night light in the Better OS session

**Spec:** `design/system-settings-spec.md` section 9, F4; ADR 0042 (proposed)
**Blocked by:** the owner's decision M5, and a compositor that offers gamma control, and the owner's decision M1

## Why

`cosmic-comp` 1.11 has no gamma or colour-temperature protocol, so nothing
can tint the screen in the session; the Displays page says so (ticket 235).

## What to build

- With M5 (b): at each Epoch release ticket 226's routine checks whether
  `cosmic-comp` offers gamma control or a night light of its own; when it does,
  a night-light service or COSMIC's own configuration behind the same card,
  with the same copy.
- With M5 (a): a new ADR widening ADR 0039, the protocol in ticket 223's patch,
  and a Rust night-light service; this ticket then becomes large.

## Done when

- [ ] The Displays page's night light card works in the session with the
      copy of section 9.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/251/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
