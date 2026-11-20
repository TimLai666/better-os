# Ticket 221 — Better Input types in the Better OS session

**Spec:** better-os-session.md S5 (Better Input row); `design/better-input-spec.md`
**Blocked by:** 185, 203, and the owner's decision S4

## Why

C4 makes Fcitx 5 with Chewing the session's input method, because IBus
1.5.29 offers `cosmic-comp` no Wayland input method (ADR 0038). Better Input
(tickets 180 to 191) is designed as an IBus engine under GNOME. In the session
the same engine core must reach applications through something other than
IBus, or the owner's input method stops at the session's edge.

## What to build

- **First, settle decision S4's facts** and record them in Notes: whether
  Fcitx 5 can host an engine running in another process (as IBus does over
  D-Bus), from its source and documentation at the version Ubuntu 24.04
  ships; what XWayland applications in the session use (XIM through Fcitx 5);
  whether `cosmic-comp` lets a second client bind `zwp_input_method_v2` while
  Fcitx 5 holds it. Bring the answer to the owner with the options of S4 and
  stop until S4 is answered.
- **Then the chosen front end** for `input-core` in the session (S4 (c): an
  adapter to Fcitx 5's out-of-process interface; S4 (a): a Rust
  `zwp_input_method_v2` and virtual-keyboard client, with Fcitx 5 not started
  while Better Input is the input method and an XIM path for X11 clients),
  with composition, candidates, dictation (ticket 187), and the update rule of
  ticket 183 (D9) carried over.
- **Install and removal in the session**: ticket 185's install order with the
  session's framework in place of the IBus restart, and ticket 184's
  input-source entry as the Fcitx 5 profile writer of ticket 208.
- **The dictation shortcut** (ticket 187) through ticket 206's adapter in the
  session, checked against the session's shortcuts before applying.

## Done when

- [ ] The facts and the owner's S4 answer in Notes.
- [ ] Protocol tests for the chosen front end against a fake.
- [ ] In the VM session, Better Input types and dictates into a Better OS
      window, a GTK 4 window, Firefox, and an Electron application, and an X11
      application on XWayland, and removing it puts Chewing back; screenshots
      in `/home/tim/Developer/better-os-evidence/221/`.
- [ ] fmt, clippy on stable, the crates' tests, and `verify-deb.sh` pass.
