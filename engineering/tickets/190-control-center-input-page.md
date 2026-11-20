# Ticket 190 — The control center's Input page

**Blocked by:** 180, 185, 186

## Why

Better Input's settings live in Better Manager like every other component's
(ADR 0017); the page sits beside Default apps (ticket 89). Spec section 7 gives
every card, state, and string in both languages; the mockups are
`design/better-input/page-*.html` and `voice-setup-*.html`.

## What to build

- **Navigation**: the item 輸入法 / Input with the `Keyboard` icon below
  預設應用程式 / Default apps, `better-manager --page input`, and the "Find a
  setting" entries for every card.
- **Every card and state of spec section 7.1–7.5**, in its group, with its
  control, its default, and its zh-TW and English copy, reading and writing
  through `input-core`'s settings and the voice service's interface (never a
  file directly): status and the gate's notice, Typing, Personal with the My
  words sub-page (list, search, add with the reading filled from the lexicon,
  remove, export, import, empty state), Voice input with the model card in every
  state (downloading, metered, failed, corrupt, not enough space), the voice-key
  test, the shortcut picker, hold to talk with its caption, microphones from
  PipeWire, where it runs (the NPU entry disabled until ticket 191), and About
  with the licences.
- **The rules between settings**: toneless and mixed excluding each other,
  Space selects disabled in mixed mode, voice off deleting the model after the
  caption says so, each with the caption the spec gives.
- The shared states (not installed, too old, service not running) from the
  control-center spec section 2.
- The new kit strings of spec section 8.

## Done when

- [ ] Unit tests in the page's GPUI-free model for every state and rule above,
      in both languages.
- [ ] In the headless sway sandbox (AGENTS.md's rules) against fakes of the
      engine settings and the voice service: screenshots of the page in its
      default state and of the voice group while downloading, light and dark,
      zh-TW and English, in `/home/tim/Developer/better-os-evidence/190/`,
      compared with the mockups; differences fixed or recorded.
- [ ] fmt, clippy on stable, the crates' tests pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

The page gains the session's state: in the session it names the framework Better Input runs in (Fcitx 5, or Better Input's own client under S4 (a)) and shows no IBus state; the strings are added with ticket 221.
