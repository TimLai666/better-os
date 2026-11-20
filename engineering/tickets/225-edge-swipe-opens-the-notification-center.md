# Ticket 225 — Two fingers from the right edge of the touchpad open the notification center

**Spec:** better-os-session.md S6 ("The edge swipe")
**Blocked by:** 216, and the owner's answer to ticket 161's N3

## Why

macOS opens its notification center with two fingers moving left from the
right edge of the trackpad. libinput reports a touchpad swipe as the fingers'
centre only, so neither a Wayland protocol nor `cosmic-comp` can tell an edge
swipe from any other (ADR 0038). The only Better OS code that sees where the
fingers are is the scroll pass-through (ADR 0030), ticket 161's option (a),
the same on GNOME and in the session.

## What to build

(As ticket 161's option (a), if the owner chooses it; if N3 is (b) or (c),
this ticket is rewritten or withdrawn with the owner's answer.)

- **An ADR** for the pass-through holding the pad whenever the edge swipe is
  on (today it holds it only while a scroll factor other than 1 is set), and
  the security review `engineering/specs/touchpad-input-passthrough-security.md`
  updated in the same change.
- **Edge detection** in `touchpad-scroll`'s library behind its device seam:
  two contacts starting within the edge band of the pad's right side moving
  left; the scroll suppressed for that stroke only; progress reported to the
  gesture service as a new typed event, and from there `TransitionUpdate` for
  kind notification center (ticket 224's path).
- On GNOME, the center does not exist until ticket 161's GNOME build; the
  edge swipe is off there and the page says why.

## Done when

- [ ] The ADR and the security review updated, accepted by the owner.
- [ ] Unit tests over recorded evdev frames: edge strokes, strokes starting
      inside the band and leaving it, three fingers, palms, a scroll that
      starts near the edge and goes vertical.
- [ ] In the VM with a replayed evdev recording, the center follows the
      fingers and settles; on the owner's machine in ticket 227.
- [ ] fmt, clippy on stable, and the crates' tests pass.
