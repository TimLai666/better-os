# 39 — Touchpad advanced customization

**Epic:** Better Touchpad (Issue #3) phase 4
**User Story:** A user maps a five-finger gesture to a custom keyboard
shortcut, keeps different gesture profiles per device, and moves their setup
between machines with export and import.
**Blocked by:** 30 (38 for live invocation, not for the models)
**Status:** todo

## What it delivers

- Custom contact counts including five-finger mappings, editable in the
  Gestures screen (the model already bounds 1–5; the preset stays
  thumb-plus-three per the issue's decision).
- Custom keyboard-shortcut actions: the `better-actions` validated
  `KeyboardShortcut` becomes assignable from the gesture editor with a
  capture-style picker (typed key list, no free text reaching execution).
- Per-device gesture profiles: gesture configuration keyed by the stable
  device identity `touchpad-platform` already builds, with a global fallback
  profile; switching devices switches the active profile; ADR 0010's
  per-session limitation note updated to distinguish pointer/scroll settings
  (still session-global) from gesture profiles (now per-device).
- Import and export: a versioned profile document (config schema reuse, not
  a new format) written to and read from a user-chosen path; untrusted on
  import — full validation, version migration, and a preview-and-confirm
  apply through the existing plan gate before anything changes.
- GUI: editor affordances for the above, zh-TW/en-US, overflow tests.

## Out of scope

- Arbitrary shell commands (permanently excluded).
- Cloud sync of profiles.

## Verification

Workspace gates; import rejection suite (malformed, wrong version, hostile
values); per-device profile switching tests; export/import round-trip byte
and semantic equality; locale overflow tests; headless GUI smoke.
