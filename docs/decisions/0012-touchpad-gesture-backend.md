# ADR 0012: The production gesture backend for Better Touchpad

## Status

Accepted for ticket 30, and **amended on 2026-08-31**: the project owner granted
the GJS language-policy exception this decision was conditional on, and ticket 38
built the adapter. The amendment is at the end of this document, under
[The exception, as granted](#the-exception-as-granted). Everything above it is
the decision as it stood when it was taken, kept unedited so that a later reader
can see what was decided before the grant and what changed because of it.

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

## The exception, as granted

Amendment, 2026-08-31. The project owner granted the bounded GJS exception.
Ticket 38 built the adapter, and this section records what was granted, what was
built inside it, and what is still open — including the one bound that had to be
widened to make the decision work, which is the part a later reader should read
first.

### What the exception permits

JavaScript is permitted in Better OS in exactly one place: the GNOME Shell
adapter extension under `adapters/gnome-shell-touchpad/`. Inside it, JavaScript
may do two things — report what the compositor saw, and perform an action GNOME
Shell already exposes. It may not hold a threshold, a cooldown, a cancellation
rule, a conflict decision, a configuration value, or any gesture decision, and it
may not execute anything. `AGENTS.md` carries the same sentence as a rule, and
`crates/touchpad-session/tests/extension.rs` asserts the file against it, so
widening the exception fails a test rather than passing a review.

### The bound that was widened, and why

The ADR above says the adapter "publishes exactly one D-Bus interface, which
**emits signals and accepts no method that changes anything**". That bound was
wrong, and ticket 38 changed it deliberately rather than quietly.

A signal-only adapter can report a gesture and cannot open the overview, because
opening the overview is `Main.overview.show()` inside the shell's own process and
there is no other route to it. Keeping the bound would have delivered a gesture
pipeline that recognizes every gesture in the preset and performs none of them.
So the interface now has four methods, and the bound that replaces it is a
sharper one: **every method is an action GNOME Shell already performs, named as
itself, with no free-text argument.** `ShowOverview`, `ShowDesktop`,
`SwitchWorkspace(direction)`, and `SuppressBuiltInGestures(suppress)` are the
whole list, and `Capabilities` reads and changes nothing. There is no method that
takes a command, a path, a key, or a gesture, which is the property the original
bound was reaching for.

### What GNOME 46 could and could not do

Recorded because these are the facts the design rests on, and a later GNOME may
change any of them:

- **A thumb is invisible.** Clutter's touchpad gesture events carry a contact
  count and nothing about which contact is which. The capability report says
  `thumb_detection: false`, and `touchpad_gestures::ingest::assumes_thumb`
  assumes a thumb only where every gesture of that kind and count wants one — so
  the preset's thumb-and-three pinch works, and it would stop working the moment
  a four-finger pinch was configured beside it, which is the honest behaviour
  rather than a coin toss between the two.
- **The current application's windows have no facility.** GNOME 46's window
  picker is the overview itself and cannot be filtered to one application. The
  action is reported unsupported with that reason. This is the fourth row of the
  Mac-style preset, and it does not work on GNOME 46.
- **Progress is continuous, and actions are discrete.** The event stream carries
  begin, update, end, and cancel with cumulative deltas, so the recognizer sees
  every frame and a gesture reversed before its threshold never reaches the
  desktop. But GNOME 46 exposes no supported way to drive the overview's own
  transition from outside the shell, so the action fires at the end. Issue #3
  allows discrete activation as the first fallback; the architecture does not
  prevent the other, because the progress already reaches the adapter.
- **The shell's own trackers can be turned off by their own property.**
  `Main.overview._swipeTracker` and `Main.wm._workspaceAnimation._swipeTracker`
  both have an `enabled` property. Suppression saves what each was and puts it
  back, and the extension restores on `disable()` whatever it was last asked, so
  an uninstall cannot leave the desktop without its gestures. The capability
  report carries how many trackers were found, so a shell that renamed them
  reports zero rather than reporting a suppression that did nothing.

### What was verified, and how

One nested `gnome-shell --nested --wayland` under `dbus-run-session`, with
`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, and `XDG_CACHE_HOME` all
pointed at a temporary directory, so the extension was never installed into the
host session and no host dconf key was written. On GNOME Shell 46.0 the extension
enabled, owned its bus name, reported two built-in swipe trackers, flipped
`built_in_gestures_suppressed` to true and back, and answered all four action
methods without error.

What that run did **not** cover is the gesture signals: a nested shell with no
touchpad input produces none, so the swipe and pinch event path has been driven
end to end only against a Rust fake of the same interface over a private session
bus. The recognition, the thresholds, the cooldown, and the actions are therefore
tested; the compositor's own event shape is taken from the Clutter API and has
not been observed against a hand.

### What is still open

1. The libinput security review is still owed, and path C is still the better
   long-term answer for X11 and non-GNOME sessions.
2. The hardware and release matrix ADR 0008 asked for still does not exist.
   Nothing here has been tested against GNOME 47, 48, or 49, and
   `conflict::GNOME_46_GESTURES` remains a static model.
3. `ingest::EventScale::swipe_pad_pixels` is a recorded starting value of 1000,
   in the same sense as the thresholds above it: a compositor reports swipe
   motion in pixels and never says how big the pad is, and nothing has measured
   this against a hand.
