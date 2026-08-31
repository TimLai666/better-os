# 39 — Touchpad advanced customization

**Epic:** Better Touchpad (Issue #3) phase 4
**User Story:** A user maps a five-finger gesture to a custom keyboard
shortcut, keeps different gesture profiles per device, and moves their setup
between machines with export and import.
**Blocked by:** 30 (38 for live invocation, not for the models)
**Status:** done

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

## What was built

- **Contact counts.** The editor already offered one to five; what was missing
  was a test that every one of them saves, that six is refused rather than
  clamped, and that a five-contact thumb gesture draws five dots. The preset is
  untouched — thumb plus three is still what the launcher and Show Desktop map.
- **Custom keyboard shortcuts.** `gestures_model::ShortcutDraft` is a modifier
  set and one `better_actions::Key`, and `KeyGroup` splits the seventy-three-key
  table into six pickable parts, asserted to cover it exactly once. There is no
  text field in the path: a gesture's shortcut is always rebuilt from the draft
  on save, and a draft with no modifier is refused with the reason left in the
  editor. `touchpad_platform::keybindings` reads the recorded `wm`, media-key,
  and shell bindings out of the user's own dconf database through
  `defaults-platform`'s GVDB parser, and `touchpad_gestures::shortcut` compares
  them. The answer is one of three — collides with this key, nothing recorded
  matches, or could not be read — and the wording says GNOME's compiled-in
  defaults are not readable, so "nothing recorded" is never drawn as "clear".
- **Per-device gesture profiles.** `gestures.json` is schema version 2:
  `GestureProfiles` holds a global profile, profiles keyed by
  `touchpad-platform`'s stable identity, and the selected identity. Version 1 —
  a bare configuration — migrates on read. The active profile follows the pad
  the rest of the window is about. A pad with no profile of its own *follows*
  the global one, so opening the window or verifying a binding cannot make it
  diverge; divergence is its own button, and so is going back.
- **Export and import.** The same document, written to and read from a path the
  user names. Import is untrusted: full validation, version migration, bounded
  sizes, device identities checked against the two shapes the platform builds,
  and every bounded value re-checked, so no shell string, impossible contact
  count, or out-of-range threshold survives. An imported document reaches a
  binding only through `PresetPlan::approve` — the same conflicts-then-confirm
  gate the preset uses, with no second apply path.

## Known limits

- A recorded binding whose spelling is outside the fixed key table (a media key,
  a keypad key) cannot be compared and is counted rather than guessed at, so the
  collision check can miss a collision with one of those.
- The capture (`gestures-backup.json`) is still one file for the machine, not
  one per profile. Restore puts it back into the profile in force.
- Nothing routes a recognized gesture to the profile of the pad that produced
  it. Merged with ticket 38, `better-touchpad-gestured` recognizes against the
  profile in force rather than always the global one, but the GNOME Shell
  adapter reports no device identity, so on a machine with two touchpads both
  pads perform the selected pad's profile.
