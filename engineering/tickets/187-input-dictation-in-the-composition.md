# Ticket 187 — Dictation in the composition: start, stop, hold, correct

**Blocked by:** 183, 186, and the owner's decision D6 in
`design/better-input-spec.md`

## Why

Ticket 99 asks for push-to-talk and a toggle, with dictated text streaming into
the same composition so it can be corrected by typing. Spec sections 4.1, 4.2,
5.4, 5.5, and 7.6 describe it.

## What to build

- **The engine side** of spec section 4.1: a dictated segment at the cursor,
  stable words in the preedit, the tentative tail and the listening hint in the
  auxiliary text, the final pass replacing the segment, ↓ on a dictated word
  offering the recogniser's alternatives and libchewing's homophones, Esc
  cancelling while listening, any typing key stopping listening first, the
  property symbol 聽 while listening.
- **Start and stop**: the voice key (ticket 188's `Pressed`, debounced), press
  to start and press to stop; Enter, Esc, typing, or the silence timeout from the
  setting ends it; refused with the spec's notifications when no Better Input
  context is focused, in a password field, or while the model is still
  downloading; nothing recorded when the session is locked or not active
  (logind).
- **Hold to talk** (decision D6, off by default): Right Alt held 300 ms with no
  other key starts listening, release stops; Alt+key combinations unaffected.
- **The dictation shortcut** through Better Defaults (`global-shortcut`):
  `<Super><Alt>h` running `better-input-voice --toggle`, checked against the
  wm, shell, mutter, and media-keys schemas before applying, restored on turn
  off and removal.
- The top bar shows the state through the engine's own property, so ticket 157
  needs no change; confirm it in the sandbox if ticket 157 has landed.

## Done when

- [ ] Unit tests in `input-core` for the dictated segment through partials and
      the final, correction by every editing key, alternatives under ↓, Esc,
      typing during dictation, the silence timeout, and the refusals.
- [ ] Hold-to-talk tests with Right Alt alone, Right Alt with Tab, and a short
      press.
- [ ] In the headless GNOME Shell sandbox with the built package: a fixture WAV
      through a fake audio source dictated into a GTK 4 window, corrected by
      typing, committed with Enter; screenshots light and dark in
      `/home/tim/Developer/better-os-evidence/187/`, compared with
      `design/better-input/dictation-*.html`.
- [ ] fmt, clippy on stable, the crates' tests pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

The engine side and the dictation shortcut are per session. In the session the composition reaches applications through ticket 221's front end, and the shortcut `<Super><Alt>h` is written through the cosmic-config adapter (ticket 206), checked against the session's own key bindings rather than GNOME's wm, shell, mutter, and media-keys schemas; ticket 221 carries both. This ticket is unchanged for GNOME.
