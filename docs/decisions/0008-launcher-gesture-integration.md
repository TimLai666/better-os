# ADR 0008: Better Launcher Five-Finger Gesture Integration

## Status

Accepted as a deferral for ticket 21. The production mechanism is chosen and
built under ticket 30.

## Decision

Better Launcher ships a typed gesture adapter boundary and no production
adapter. `launcher-platform::gesture` defines the event shape
(begin/update/end/cancel, inward/outward, progress, finger count), the
threshold and cooldown configuration, and a recognizer that turns samples into
open, close, progress, and cancel outcomes. The only adapter in this build is a
mock one, and `SessionCapabilities` reports `NoAdapter` on every real session.

The four integration paths Issue #2 names are compared below. None is adopted
yet, because choosing one requires evidence this ticket cannot produce: how
GNOME 46 through 49 actually behave on the target releases, whether a five-
finger gesture reaches an unfocused application at all, and what a security
review says about the one path that would work everywhere.

What is decided now:

- The gesture never becomes the only way in. The desktop entry and the
  configurable global keyboard shortcut are the required paths, and a machine
  with no gesture support shows a shorter list of activation paths rather than
  an error.
- No launcher behavior — matching, ranking, the application list, the window —
  may live behind the adapter. An adapter reports fingers, direction, and
  progress. It cannot open the launcher; the recognizer decides that, in Rust,
  in this repository.
- No path that requires the GPUI process to run as root, or that grabs input
  devices globally without an explicit security review, is eligible.

## Constraints, from Issue #2

1. No launcher behavior or ranking logic in a GNOME Shell extension.
2. The GPUI process does not run as root.
3. No global raw-input device grab without an explicit security review.
4. No silent permanent GJS exception to the Rust-only language policy. If a
   minimal GJS adapter turns out to be the only practical GNOME path, it is
   documented as an explicit exception and limited to emitting open, close, and
   progress events.
5. Unsupported touchpads degrade cleanly to keyboard and desktop-entry
   activation.
6. Continuous gesture-driven animation is desirable but must not block a
   keyboard-operable launcher.

## The four paths

### A. Minimal GNOME Shell adapter (GJS extension)

A small extension binds a five-finger swipe through Shell's own gesture stack
and emits events over D-Bus to the launcher.

- **Works because** GNOME Shell is the only component on a GNOME Wayland
  session that sees touchpad gestures before the focused application does. It
  is the one path that can deliver a five-finger pinch while another
  application is focused, which is the whole point of the gesture.
- **Continuous progress:** yes. Shell's gesture tracker exposes progress, so
  the animation Issue #2 wants is reachable.
- **Costs:** it is JavaScript, which the language policy forbids without an
  explicit exception (constraint 4). Extensions break on every GNOME major
  release and are disabled by default after an upgrade, so the gesture would
  silently stop working exactly when a user upgrades. It also puts a Better OS
  component inside the Shell process, where a fault is a session fault.
- **Security:** no new privilege. The extension runs as the user, inside a
  process that already sees all input. The exposure is the D-Bus interface it
  publishes, which must be limited to emitting events and must accept nothing.
- **Verdict:** the only currently viable GNOME Wayland path, and the one that
  costs a language-policy exception. Not adopted without that exception being
  taken deliberately.

### B. Compositor-supported gesture integration

Mutter, or a Wayland protocol it implements, delivers the gesture to a client
that asked for it.

- **Works because** it would be the correct place for this to live: the
  compositor already arbitrates gestures between the shell and applications.
- **Continuous progress:** yes, in principle.
- **Costs:** the capability does not exist today. `pointer-gestures-unstable-v1`
  delivers swipe and pinch to the *focused* surface only, which is useless for
  opening something that is not on screen. Adopting this path means writing a
  protocol proposal and shipping nothing until it lands upstream.
- **Security:** best of the four. No extra privilege, no extra process, and the
  compositor stays the arbiter.
- **Verdict:** the right long-term answer and not an option for this release.
  Worth tracking rather than building.

### C. Rust/libinput session service

A user-session daemon reads touchpad events from libinput and recognizes the
gesture itself.

- **Works because** it is entirely ours, in Rust, on every desktop and both
  display protocols. No shell, no compositor, no extension.
- **Continuous progress:** yes, and with the most control of the four.
- **Costs:** reading `/dev/input/event*` requires membership in `input` or a
  privileged helper. That is a global raw-input grab in everything but name:
  a process that can read the touchpad can read the keyboard, so a fault or a
  compromise in it is a keylogger. Constraint 3 makes this conditional on an
  explicit security review. It also duplicates gesture recognition the
  compositor is already doing, and would fight the compositor for the same
  gesture on GNOME.
- **Security:** the worst of the four, and the reason this ADR exists rather
  than a commit. The review would have to cover what the service reads, what it
  keeps, what it exposes, how it is confined, and what happens when it crashes.
- **Verdict:** viable but expensive, and not to be started before the review
  question is answered rather than after.

### D. Desktop portal or other standard protocol

`org.freedesktop.portal.GlobalShortcuts` or a future gesture portal.

- **Works because** it is the sanctioned way for an application to receive
  input it did not focus, with the user consenting once.
- **Continuous progress:** no. GlobalShortcuts is a trigger, not a gesture: it
  delivers activation, not a progress stream, so the animation Issue #2 wants
  is out of reach on this path.
- **Costs:** GNOME's implementation is aimed at key combinations, and there is
  no gesture portal at all. It cannot express "five fingers inward" today.
- **Security:** best-in-class for what it does cover. The user grants the
  shortcut in the portal's own dialog and can revoke it.
- **Verdict:** not a gesture path today. It is, however, the path worth
  revisiting for the *keyboard* shortcut, which this build currently expresses
  as GNOME custom-keybinding settings applied by Better Defaults.

## Comparison

| Path | Works on GNOME Wayland today | Continuous progress | Language policy | Security cost |
| --- | --- | --- | --- | --- |
| A. GNOME Shell adapter | Yes | Yes | Needs an explicit GJS exception | Low, plus a new D-Bus surface |
| B. Compositor integration | No, protocol does not exist | Yes | Clean | Lowest |
| C. Rust/libinput service | Yes | Yes | Clean | Highest — needs a security review |
| D. Portal / standard protocol | No gesture portal exists | No | Clean | Low |

## Why the boundary is built now anyway

Two things would be much harder later. Every path above produces the same three
facts — direction, progress, and whether the gesture completed — so the event
shape can be settled before the mechanism is. And a recognizer that lives in the
launcher rather than in the adapter is what keeps constraint 1 true: whichever
path wins, the decision to open the launcher is made in Rust, in this
repository, under the threshold and cooldown policy tested here.

The recognizer takes the current time as an argument rather than reading a
clock, so the cooldown that stops an accidental partial gesture from flapping
the overlay is replayable in a test instead of being timing-dependent.

## Still deferred

- Which path ships. Ticket 30 decides, with a hardware and release matrix.
- Whether a minimal GJS adapter is an acceptable exception. That is a policy
  decision, not an engineering one, and it belongs to whoever owns the language
  policy in AGENTS.md.
- The animation style and how gesture progress maps to it. The boundary carries
  progress; nothing consumes it yet.
- The exact global keyboard shortcut. `launcher-platform::shortcut` names the
  GNOME settings that would carry it and deliberately leaves the binding unset.
