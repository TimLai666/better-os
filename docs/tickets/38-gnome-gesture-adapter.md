# 38 — GNOME Shell gesture adapter and live gesture pipeline

**Epic:** Better Touchpad (Issue #3) phase 3
**User Story:** The Mac-style preset actually works on a Zorin/GNOME Wayland
session: thumb-plus-three pinch opens Better Launcher, four-finger swipes drive
the desktop, and reversing a gesture before the threshold cancels it.
**Blocked by:** 30
**Status:** done

## Decision this ticket carries

The project owner granted the bounded GJS language-policy exception ADR 0012
required (2026-08-31). Amend ADR 0012 to record the grant and its bounds, and
update `AGENTS.md`'s language rule: GJS is permitted only inside the Better
Touchpad GNOME Shell adapter extension, only for bridging typed events and
actions, never for gesture logic, thresholds, configuration, or actions
beyond invoking what GNOME Shell already exposes.

## What it delivers

- A minimal GNOME Shell extension (GJS) under `adapters/gnome-shell/` (or a
  similarly explicit path): captures compositor touchpad gesture events
  (swipe and pinch begin/update/end with finger count and deltas), forwards
  them over the session bus as typed signals, and exposes typed desktop
  actions GNOME Shell owns — overview, show desktop, current-application
  windows, workspace switch — as D-Bus methods. No thresholds, no
  configuration, no shell command execution, no gesture decisions in GJS.
  While the Better OS gesture pipeline holds the preset active, the
  extension suppresses the built-in gestures it replaces and restores them
  on disable, per the conflict plan the user confirmed in the GUI.
- A resident Rust gesture pipeline (extend `touchpad-session` with a small
  user service or a session component owned by the existing crates — record
  the choice): consumes the extension's event stream, runs the existing
  `touchpad-gestures` recognizer/config, invokes actions through
  `better-actions` (launcher via `org.betteros.Launcher1`, desktop actions
  via the extension's methods), honors thresholds, cancellation, cooldown,
  and continuous progress where the event stream carries it.
- Continuous progress: launcher open and overview follow gesture progress
  where the adapter exposes it; discrete activation is the documented
  fallback.
- Capability honesty: thumb detection is reported per what the compositor
  events can actually distinguish; if a thumb-plus-three gesture is only
  observable as four contacts, the capability says so and the preset matches
  on four contacts with `thumb_required` marked best-effort.
- Verification and safety: per-gesture verification results surfaced in the
  GUI; the repeatedly-failing-adapter auto-disable from ticket 30 wired to
  the real adapter; safe mode disables the extension bridge; uninstall or
  disable restores the built-in gestures.
- Packaging: the extension ships in the `better-touchpad` package (installed
  under `/usr/share/gnome-shell/extensions/`), enabling stays a user/manager
  step; manifest updated (paths, health check for the adapter bridge).
- Tests: recognizer-to-action integration over recorded/synthetic event
  streams through the real D-Bus service against a private session bus (the
  house pattern); extension JS validated by syntax check and a schema test
  of its D-Bus interface XML; conflict suppress/restore state machine tests.
  A real nested-shell or live-session run is attempted only if it needs no
  host mutation; otherwise the gap is recorded honestly.

## Out of scope

- Patching Mutter or GNOME Shell source; LD_PRELOAD; raw `/dev/input`.
- X11 gesture depth (deferred decision).
- Two-finger application-level back/forward/zoom/rotate (no adapter path).

## Verification

Workspace gates; private-session-bus integration suite; `bash -n`/`gjs`
syntax validation for the extension; packaging build+verify including the
extension payload.

## What was built, and what it is

The design decisions this ticket had to make, recorded here because the ticket
left them open.

**The extension lives in `adapters/gnome-shell-touchpad/`** and is three files:
`metadata.json` (uuid `touchpad-adapter@betteros.org`, shell-version 46),
`extension.js`, and `org.betteros.TouchpadAdapter1.xml`. The XML is the single
source of truth for the interface — the extension loads it at enable time rather
than embedding a second copy, and a Rust test asserts the client and the file
name the same members.

**The service is its own crate and its own binary.** `touchpad-gesture-service`
produces `better-touchpad-gestured`, rather than a mode of `touchpad-gui` or a
binary inside `touchpad-session`. Two reasons, in order: a gesture that only
worked while the settings window was open would not be a gesture anyone could
rely on, and `touchpad-gestures` already depends on `touchpad-session`, so the
pipeline — which needs both — has to sit above them. It links no toolkit, opens
no window, and blocks on the extension's signal stream between gestures.

**The event ingestion path extends the recognizer rather than forking it.**
`touchpad_gestures::ingest` turns each compositor event into the contact frame it
would have been: the contacts a gesture of that many fingers starts from, moved
by exactly what the compositor reported. Every threshold, the arming rule, the
reversal rule, the value-at-release rule, the cooldown, and the frame-health
counters are the recognizer's own, untouched. The one addition to the recognizer
is `Recognizer::cancel`, for the case where the compositor itself says a gesture
was cancelled and there is no frame that could express it.

**`current-application windows` maps to nothing on GNOME 46.** The window picker
*is* the overview and cannot be filtered to the focused application, so there is
no method for it in the extension and the action is reported unsupported with
that reason. The four-fingers-down row of the preset does not work on this shell.

**Thumb detection is unsupported and the preset still works.** Clutter carries a
contact count and nothing about which contact is which. `ingest::assumes_thumb`
assumes a thumb only where every gesture of that kind and contact count wants
one, which covers the preset's four-contact pinch and spread and nothing else. A
four-finger pinch configured beside the launcher gesture would make both
unmatchable, which is the honest outcome rather than a coin toss.

**Actions are discrete.** The event stream is continuous and the recognizer uses
every frame of it — a gesture reversed before its threshold never reaches the
desktop at all — but GNOME 46 exposes no supported way to drive the overview's
own transition from outside the shell. Progress is forwarded to the adapter on
every phase, and the adapter reports the intermediate ones ignored.

## Verification

Workspace gates, plus:

- `crates/touchpad-gesture-service/tests/pipeline.rs`: nine tests over a private
  `dbus-daemon` with a Rust fake serving the real interface. Recorded gesture
  streams go in as signals; typed method calls come out. Covers the overview
  action, progress forwarding, cancel-on-reverse, the cooldown, the launcher
  route, suppress and restore across all four ways out, auto-disable after three
  failures with the configuration written, verification results written where the
  window reads them, and an unreadable signal being dropped.
- `crates/touchpad-session/tests/extension.rs`: the interface XML against the
  Rust client's member list, the signal argument shapes, the packaged uuid, the
  bounds of the language exception asserted against the JavaScript, and `gjs -m`
  parsing `extension.js` (skipped with a printed reason where `gjs` is absent).
- `crates/touchpad-gestures/src/ingest.rs`: twelve tests over the event-level
  path, including that a whole compositor swipe is the overview gesture, that a
  short one is not, and that the cooldown behaves as it does on replayed frames.
- `crates/touchpad-gestures/src/suppression.rs`: the state machine, including
  that a failed restore is retried rather than assumed.

### The live-shell run

One nested `gnome-shell --nested --wayland` under `dbus-run-session`, with
`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, and `XDG_CACHE_HOME` all
pointed at a temporary directory, so nothing was installed into the host session
and no host dconf key was written. Reproduce it with those four variables set to
absolute paths under a temporary root, the extension copied to
`$XDG_DATA_HOME/gnome-shell/extensions/touchpad-adapter@betteros.org`, then
`gnome-extensions enable` and `gdbus call` against the interface. A relative path
in any of the four silently disables the isolation and the shell will not find
the extension.

On GNOME Shell 46.0 the extension enabled, owned `org.betteros.TouchpadAdapter1`,
reported both of the shell's swipe trackers, flipped
`built_in_gestures_suppressed` to true and back, and answered `ShowOverview`,
`ShowDesktop`, and `SwitchWorkspace` without error.

**What that run did not cover:** the gesture signals. A nested shell with no
touchpad input produces none, so the swipe and pinch event path has been driven
end to end only against a Rust fake of the same interface. The event *shape* is
taken from the Clutter API rather than observed against a hand, and
`EventScale::swipe_pad_pixels` is an untuned starting value.
