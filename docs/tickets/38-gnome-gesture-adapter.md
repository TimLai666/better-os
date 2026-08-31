# 38 — GNOME Shell gesture adapter and live gesture pipeline

**Epic:** Better Touchpad (Issue #3) phase 3
**User Story:** The Mac-style preset actually works on a Zorin/GNOME Wayland
session: thumb-plus-three pinch opens Better Launcher, four-finger swipes drive
the desktop, and reversing a gesture before the threshold cancels it.
**Blocked by:** 30
**Status:** todo

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
