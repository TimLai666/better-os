# ADR 0012: The production gesture backend for Better Touchpad

## Status

Accepted for ticket 30. This ADR records the choice and its conditions; ticket
30 deliberately implements none of it. Phase 3 builds what is chosen here.

## Context

Issue #3 requires that the production gesture backend be selected through an
ADR comparing viable options rather than by whichever path turned out to be
easiest to write. [ADR 0008](0008-launcher-gesture-integration.md) already
compared the same four paths for Better Launcher's five-finger gesture and
deferred the choice to this ticket, "with a hardware and release matrix". That
matrix does not exist and cannot be produced from this machine, which is the
first thing this ADR has to say honestly, because it changes what can be
decided today and what cannot.

What ticket 30 built is everything above the backend: the gesture model, the
Mac-style preset, conflict detection, the recognizer, the typed action catalog,
and the session-adapter boundary. All of it is tested by replaying frames, none
of it knows a compositor exists, and a backend is a producer of frames or of
gesture progress plugged in front of it.

## Decision

**Better Touchpad ships the minimal GNOME Shell adapter first — path A — as an
explicit, bounded exception to the Rust-only language policy, and only once the
exception has been taken deliberately by whoever owns that policy in
`AGENTS.md`. Until that decision is taken, Better Touchpad ships with no
gesture backend and says so on screen.**

Two supporting decisions follow from it:

- **The libinput session service (path C) is not started before its security
  review is written.** It is the better long-term answer on paper — it is Rust,
  it works on X11 as well as Wayland, and it has the most control over the
  gesture — and it is the one path where getting it wrong produces a keylogger.
  ADR 0008 already made the review a precondition. This ADR keeps it and adds
  that the review must be a written document with an owner, not a paragraph in
  a pull request.
- **Compositor integration (path B) remains the target, and is tracked rather
  than built.** If a Wayland gesture protocol that delivers a gesture to an
  unfocused client lands upstream, it supersedes path A, and the adapter
  boundary built in ticket 30 is what makes that swap cheap.

### Why path A, given that it is the one with a language-policy cost

Path A is the only one of the four that can deliver the Mac-style preset on
GNOME Wayland today. That is not a preference; it is the arithmetic:

| Path | Delivers a four-finger gesture to an unfocused surface on GNOME 46 Wayland | Continuous progress | Language policy | Security cost |
| --- | --- | --- | --- | --- |
| A. Minimal GNOME Shell adapter | Yes | Yes | Needs an explicit GJS exception | Low, plus one new D-Bus surface |
| B. Compositor / Wayland protocol | No — `pointer-gestures-unstable-v1` reaches the *focused* surface only | Yes | Clean | Lowest |
| C. Rust/libinput session service | Yes | Yes, with the most control | Clean | Highest — a process that can read the touchpad can read the keyboard |
| D. Portal or other standard protocol | No — there is no gesture portal, and `GlobalShortcuts` is key combinations | No | Clean | Low |

Two of the four cannot do the job at all today, and the choice is therefore
between a JavaScript exception and a raw-input security review. The exception is
smaller, reversible, and bounded by what the adapter is allowed to contain; the
review is a piece of work nobody has started and a risk nobody has accepted.

### What the adapter is allowed to be

The exception is worth taking only if it stays this small. The adapter:

- binds swipe, pinch, and spread gestures through GNOME Shell's own gesture
  stack and emits `begin`, `update`, `end`, and `cancel` with a progress
  fraction, a contact count, and — where Shell can tell — which contacts are
  thumbs;
- publishes exactly one D-Bus interface, which **emits signals and accepts no
  method that changes anything**;
- contains no thresholds, no cooldown, no conflict logic, no action, and no
  configuration. Every one of those already exists in Rust in
  `touchpad-gestures`, tested by replay, and none of it may be duplicated in
  JavaScript;
- performs no action itself. It cannot open the launcher, show the desktop, or
  switch a workspace. `touchpad-session` does that, from a typed
  `better-actions` value.

If the adapter grows a decision, the exception has been abused and the right
response is to move the decision back into Rust, not to widen the exception.

### What ships in the meantime

No gesture backend. The Gestures screen renders the preset, the conflicts, the
per-gesture rows, and the test mode against the recording adapter, and says out
loud that no adapter in this build reaches the desktop. That is not a
placeholder that pretends: `MockSessionAdapter::describe()` reports
`performs_system_actions: false`, the live-testing switch is disabled while that
is true, and the row for every unsupported action carries the reason.

The one exception is the launcher itself.
`touchpad_session::LauncherActivationAdapter` reaches Better Launcher through
the activation interface Better Launcher already serves, so the launcher action
has a real route the day a gesture backend produces the events. It reuses
`launcher-platform`'s own path rather than reimplementing it, and it reports
every other action unsupported, because that route reaches the launcher and
nothing else.

## Consequences

- `AGENTS.md` gains a language-policy exception request that is not yet granted.
  Until it is, phase 3 cannot start on path A, and this is deliberate: the
  purpose of an ADR is to make that a visible decision rather than a commit.
- A GNOME major release disables third-party extensions on upgrade. Whatever
  ships on path A must detect that its adapter is gone and say so, rather than
  silently losing every gesture. The gesture health check and the automatic
  disable rule already exist for exactly this.
- The recognizer built in ticket 30 may end up partly redundant on path A, since
  Shell also tracks gestures. That is accepted: the thresholds, the cancellation
  rule, the cooldown, and the conflict decisions stay in Rust where they are
  tested, and the adapter supplies frames or progress rather than verdicts.

## The thresholds this ticket ships

Issue #3 defers the exact thresholds and velocity curves. Ticket 30 ships them
as **recorded starting values**, in two places and nowhere else:
`GestureDefinition::DEFAULT_ACTIVATION` (0.6), `DEFAULT_CANCELLATION` (0.25),
`DEFAULT_COOLDOWN_MS` (350), and `RecognizerScale` (swipe travel 0.18 of the
pad, pinch travel 0.06, hold 500 ms, tap 200 ms, rotate 0.5 rad, arming at
0.10).

The activation threshold and the cooldown are deliberately the same numbers
`launcher-platform` already uses, so the two halves of the launcher gesture
commit at the same place. None of them has been measured against a hand. They
are starting values, they are in one struct each, and changing them is one edit
and one test.

## What would change this decision

Recorded so that a later reader can tell whether the decision still holds:

1. **A written security review of the libinput path with an accepted risk
   position.** That makes path C available, and it is the better answer for a
   product that wants to support X11 and non-GNOME sessions.
2. **A Wayland protocol, or a Mutter interface, that delivers a touchpad
   gesture to a client that is not focused.** That makes path B available and
   supersedes path A outright.
3. **A refusal of the GJS exception.** Then path A is off the table, and
   Better Touchpad ships gesture configuration with no gesture until (1) or (2)
   happens. The GUI already renders that state honestly, which is why it is a
   survivable outcome rather than a blocked ticket.
4. **Evidence that GNOME Shell's gesture stack cannot deliver a four-contact
   pinch with a distinguishable thumb.** The Mac-style preset's two headline
   gestures depend on it. If it cannot, the preset needs a different mapping —
   five-finger gestures are already supported as custom mappings — and that is a
   product decision, not an implementation detail.

The hardware and release matrix ADR 0008 asked for is still owed. Nothing here
has been tested against GNOME 47, 48, or 49, and the GNOME 46 gesture model in
`touchpad_gestures::conflict::GNOME_46_GESTURES` is a static model taken from
the shell's own swipe trackers rather than a probe, because those trackers are
compiled in and expose nothing to read. Its accuracy is a claim dated to that
release, and re-checking it is part of supporting a newer GNOME.
