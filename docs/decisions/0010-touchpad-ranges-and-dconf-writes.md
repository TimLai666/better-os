# ADR 0010: Better Touchpad ranges, configuration scope, and the dconf write path

## Status

Accepted for Better Touchpad phase 1 (ticket 29). Ticket 30 consumes the write
path and does not revisit it.

## Context

Issue #3 lists decisions that must not be made silently. Three of them block
phase 1 and are decided here: the slider ranges, per-device versus global
configuration, and X11 implementation depth. A fourth decision is not in that
list but is the larger one: how a setting is actually written.

## Decisions

### 1. The dconf write path

Better Touchpad writes GNOME settings by calling `Change` on
`ca.desrt.dconf.Writer` at `/ca/desrt/dconf/Writer/user`, over the session bus,
unprivileged.

[ADR 0009](0009-defaults-declarations-and-adapters.md) weighed three options for
Better Defaults and chose the third — read and verify fully, report manual
action required for a change — because the second needed a GVariant change set,
a D-Bus client, and a live session bus to test against, and shipping it untested
would have been worse than not shipping it. That reasoning has not changed; the
cost has been paid. Touchpad settings *are* dconf keys, so this is the ticket
where the path had to exist, and it is now built and tested against the running
service.

The first option — editing `~/.config/dconf/user` — remains rejected for exactly
the reason ADR 0009 gives: the service owns the file, caches it, and rewrites
it, so a change written behind it is ignored by the running session and
overwritten by the next write the service makes.

**The change-set encoding was taken off the wire, not out of a header.** dconf's
own client type for a change set is `(sa{smv})` — a path prefix plus relative key
names. Sending that shape is *accepted* by the service: the call returns a
change tag and no key moves. Watching what `dconf write` itself sends settles
it: the blob is `a{smv}` with absolute key paths and no prefix member. The
encoder in `crates/touchpad-platform/src/gvariant.rs` is pinned byte-for-byte
against values GLib produced, including the boundary where framing offsets widen
from one byte to two, because a hand-rolled GVariant encoder that drifts from
the specification fails exactly the way the prefix shape did — silently.

An absent value in the change set is a reset: it removes the key so the
session's own default applies again. That is what restoring a setting the user
had never touched has to do, and it is why "nothing was set here" is a distinct
reading in `touchpad-core` rather than a kind of unknown.

Alternatives rejected:

- **`gsettings`.** ADR 0005 forbids building a command line, and it would give
  back formatted strings rather than typed values.
- **A GLib dependency.** Linking GLib to build ninety bytes of a fully specified
  format, in a workspace that already has a GVDB reader of its own, is a large
  dependency for a small gain.

**Consequence for Better Defaults.** `defaults-platform`'s `DconfAdapter` can
now adopt the same path, which would turn its `gnome-keybinding` and
`gnome-desktop-setting` adapters from "manual action required" into real
applies. That is a change to Better Defaults' behaviour and its tests, so it is
recorded here as a follow-up rather than done in passing.

### 2. Slider ranges

Pointer sensitivity is `0.0 ..= 1.0` with `0.5` neutral. Scroll factor is
`0.2 ..= 5.0` with `1.0` neutral.

These are Better OS scales, not backend numbers, so that a backend change does
not move the scale under the user, and `0.5`/`1.0` mean "leave it as the session
has it" on any backend. `docs/touchpad-sensitivity-mapping.md` records the map
onto GNOME's `-1.0 ..= 1.0` speed and the three things it cannot promise.

The bounds ship as a **recorded starting point, not a settled curve**. What is
decided is that they are bounded, that they are rejected rather than clamped
when exceeded, and that they live in one place (`touchpad_core::value`). The
acceleration curve itself — whether the scale should be perceptually even rather
than linear on the backend number — needs measurement of libinput's own curve on
real hardware, which phase 1 does not do.

An alternative considered and rejected: exposing GNOME's `-1.0 ..= 1.0` directly.
It is one fewer conversion, and it would have made every screen, every stored
configuration, and every capture speak a vocabulary that is only correct while
GNOME is the backend.

### 3. Per-device versus global configuration

Phase 1 ships **one global profile**, with the selected device recorded and
displayed.

GNOME's touchpad schema is per-session, not per-device: there is one
`speed` key for every touchpad attached. A per-device configuration would
therefore be a Better OS structure that the only shipped backend cannot honour,
which is the "reports a change that never happened" failure ADR 0005 exists to
prevent.

What ships instead is everything a per-device configuration needs later, and
nothing that pretends it already works:

- device identity is stable across re-plugging (`Uniq`, or bus, vendor, product,
  version and name — never the event node);
- the configuration carries `selected_device`, which is `auto` or one identity;
- the capture records which device it was taken on;
- capability limits are read per device, so a control the *selected* pad cannot
  do is unavailable even when the backend could write the key.

The Devices screen says which scope is in force rather than implying one.

**Amended by ticket 39: this decision is about pointer, scrolling, and clicking
settings, and gestures are now out of its scope.**

The reasoning above rests on one fact — GNOME's touchpad schema has a single
`speed` key for every pad attached — and that fact is about *settings*. It does
not carry over to gestures. A gesture is recognized by Better Touchpad's own
recognizer against its own stored configuration, so a gesture profile per pad is
a structure Better OS can honour on its own rather than a promise about a key it
cannot write. Whether the *recognized event* can be attributed to a pad is a
separate question the backend answers, and the one ADR 0012 landed does not —
see the open decision below. Phase 4 therefore ships:

- **Pointer, scrolling, and clicking: still one session-global profile.**
  Unchanged, for the reason above. `config.json` keeps `selected_device` as a
  record of which pad the screen is about, not as a scope.
- **Gestures: a profile per device, with a global fallback.** `gestures.json`
  is schema version 2 from this ticket on: one global profile, zero or more
  profiles keyed by the same stable identity, and the identity selected. A pad
  with no profile of its own *follows* the global one — editing such a pad edits
  the global profile, and giving it one of its own is a separate, explicit act,
  so opening the window can never make a pad diverge by itself.

The same document is what export writes and import reads, so a file from another
machine is validated and version-migrated exactly the way a local file is, and
it reaches a binding only through the existing preview-and-confirm plan gate.

### 4. X11 implementation depth

**None.** The session type is detected and reported, and on X11 the GNOME
backend behaves exactly as it does on Wayland — because dconf and GNOME
settings are the same on both. There is no `xinput` path, no per-device X11
property writing, and no second backend.

An X11-specific backend would mean writing libinput device properties through
the X server, which is a per-device path with different verification semantics
and no overlap with the code that exists. Deciding it needs a real X11 target to
test against; Better OS targets Zorin's Wayland session.

## Consequences

- Every GNOME touchpad key Better OS names can be read, applied, and verified,
  unprivileged, with the effect immediate. Ten of thirteen controls are live;
  the other three are unavailable with a reason.
- One live, opt-in, `#[ignore]`d test changes a real setting and puts it back.
  The default suite mutates nothing.
- A capture taken before the first change is the only thing restore returns to,
  and "nothing was set" survives into it as its own state.
- Better Defaults gains a viable write path it has not yet adopted.

## Still deferred

- **The acceleration curve.** Bounded and rejected-not-clamped is decided; the
  shape of the scale is not.
- **Whether a per-device profile is worth building for pointer and scrolling
  settings** if a backend that honours one ever exists. Gestures now have one;
  the settings half still does not, because GNOME's key is still session-wide.
- **Which pad a recognized gesture came from.** The per-device profile is keyed
  by the pad the window has selected, which is the pad the user is configuring,
  and `better-touchpad-gestured` recognizes against that profile. The backend
  ADR 0012 landed reports no device identity on
  `org.betteros.TouchpadAdapter1`, so on a machine with two touchpads both pads
  perform the selected pad's profile. Routing per pad means widening that
  interface, which is an ADR 0012 change.
- **Whether Better Defaults adopts this write path**, and what its adapters then
  report for a key the user has never set.
