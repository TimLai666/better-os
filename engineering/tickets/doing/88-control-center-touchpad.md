# Ticket 88 — Control center: Touchpad

**Blocked by:** 84

## Why

Better Touchpad's window moves into the control center (ADR 0017). The audit
found a mock notice on screen (T1), English inside the Chinese UI (T2),
"requested value / effective value" wording (T3), hardware identifiers and an
English health word on the overview (T4), and a hard-coded dark theme and
misaligned controls (T5–T10).

## What to build

Spec section 5, and the Better Touchpad rows of section 12, every one of them.

- **Page** in `manager-gui/src/pages/touchpad/` with a `SelectorBar` for
  Pointer, Scrolling, Clicking, Gestures, Devices, Diagnostics; the "This
  touchpad" card replacing Overview; the staged-change Apply bar; every
  control, test area, gesture function, profile, export, import, restore,
  and safe-mode action of today's window. The GPUI-free models
  (`touchpad-gui/src/model.rs`, `gestures_model.rs`) move with their tests
  into a crate the page and nothing GPUI-bound depend on, or into the page
  module's model files; keep the assertion that no code here builds a backend
  command (`touchpad-gui/src/tests.rs`).
- **Copy:** today's tables word for word except the changes the spec lists.
- **Kit:** the sticky content-area footer for staged changes, with tests and
  a gallery page.
- **File chooser:** export and import pick a path through the desktop's file
  chooser portal (`org.freedesktop.portal.FileChooser`) over zbus; the text
  field remains as a fallback when the portal does not answer.
- **States:** not installed, too old, no touchpad, not GNOME, safe mode on,
  each as spec section 2 draws them.
- Register search entries and the Home items (safe mode on, gestures turned
  themselves off).

## Acceptance

- [ ] Each Better Touchpad row of spec section 12 is shown working by a test
      or a screenshot; capture-before-first-change and per-section restore
      keep their existing tests.
- [ ] Audit T1–T10 each closed with a screenshot.
- [ ] Light and dark, zh-TW and English, 1600 and 960, screenshotted, using
      the in-memory backend so no dconf key is written in the sandbox.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## Notes

Sub-pages are not deep-linkable (`--page touchpad` opens Pointer). Today's
`better-touchpad --page gestures` has no caller in the repository; ticket 91
drops it.

## Found on the owner's machine (2026-11-13)

Installed and enabled through Better Manager 0.2.10, the gestures still did
nothing: `better-touchpad-gestures.service` started and exited at once with
"no gesture is configured, so there is nothing to listen for", and the GNOME
Shell extension `touchpad-adapter@betteros.org` was installed but not enabled.
The owner also has `x11gestures@joseexposito.github.io` enabled. The owner's
rule is that installing makes it work, so this ticket also covers:

- Installing or enabling Better Touchpad enables the extension (through the
  same reviewed-defaults engine, restored on removal) and applies a default
  gesture profile, so one login after install is enough.
- A gesture tool that would compete (x11gestures, touchegg, fusuma) is
  detected and named, with the choice to turn it off.
- The Touchpad page lets each gesture be tried on the spot and says whether it
  reached the desktop.

## Scroll speed (owner, 2026-11-13)

The owner wants to set the touchpad's scroll speed. GNOME 46 has no key for it
(`org.gnome.desktop.peripherals.touchpad` has none) and libinput leaves it to
the compositor. Approved order: first prove, in a disposable GNOME 46 session,
whether the existing GNOME Shell extension can scale scroll deltas before
they reach clients (Clutter event handling in Mutter 46), within ADR 0012 and
0015's bounds for GJS. If it can, the Touchpad page gets vertical and
horizontal scroll-speed sliders that apply at once and are restored on
removal. If it cannot, the ticket records why with evidence and estimates the
alternative (an evdev/uinput pass-through that re-emits the touchpad with
scaled scroll) before anything is built.

## Progress (2026-11-15)

Built and tested:

- `crates/touchpad-views`: the GPUI-free models (`model.rs`, `gestures_model.rs`,
  `i18n.rs`, `startup.rs`) moved out of `touchpad-gui` with their tests; the
  no-backend-command assertion runs over it, over `touchpad-gui`, and over the
  new page module. `touchpad-gui` re-exports them and still builds and
  tests until ticket 91 drops it.
- Page `manager-gui/src/pages/touchpad/` (registered `Built`): selector bar
  (Pointer … Diagnostics), "This touchpad" expander replacing Overview,
  every control on Fluent cards (disabled with its reason when unavailable),
  pointer and scroll test areas, per-section restore, Restore all, safe mode
  switch, effective/captured value tables, the whole Gestures screen (profile
  selector, detach/forget, preset preview with per-conflict choice, confirm,
  apply, per-gesture enable/test/edit with shortcut picker and thresholds,
  live test that says whether it reached the desktop, export/import with
  preview, restore, turn off), Devices with identifiers only under Details.
- States: not installed / too old (shared states table, opens Apps ›
  Better Touchpad), no touchpad, not GNOME, safe mode on.
- Copy: spec section 5 changes (你選的/目前生效, 輕點即按一下, 尚未套用,
  health words, no language switch, no mock/adapter name); gesture problems,
  unavailable reasons, built-in gesture names and event kinds translated.
- Kit: `better_ui::fluent::staged_bar::StagedChangesBar` (tests, gallery
  section "Staged changes bar" on the Settings cards page); the Apply bar is
  pinned below the content column.
- File chooser: `pages/touchpad/portal.rs` calls
  `org.freedesktop.portal.FileChooser` over zbus; tested against a fake
  portal on a private bus; the path fields come back when the portal does not
  answer.
- Search entries for every card (Enter opens the right sub-page), Home items
  "safe mode on" (turned off in place) and "gestures turned themselves off"
  (new `gestures-turned-off` marker written by the gesture service and the
  page, cleared on re-apply or deliberate turn-off), restart hand-off and
  restart blocker for the page.

Not done yet:

- "Found on the owner's machine": enabling the GNOME Shell extension and a
  default gesture profile at install (needs the reviewed-defaults engine and
  manager-core install path, which ticket 124 is changing in parallel);
  detecting x11gestures/touchegg/fusuma with the choice to turn them off.
- Scroll speed: the disposable GNOME 46 experiment has not been run. Note for
  whoever runs it: holding a scale factor in the GJS adapter is configuration,
  which ADR 0012 forbids, so a positive result still needs a new ADR or a
  per-event round trip.
- Screenshots: taken in a wayhand sandbox that ticket 124's worker was also
  driving, so several clicks landed on its windows. Captured: not installed
  (zh light), Pointer (zh, en light 1600), Scrolling with the Apply bar,
  This touchpad expanded (T4), Gestures and its preset preview (en light, zh
  dark), Devices. Still owed: Diagnostics, Clicking, every sub-page at 960,
  en dark, safe-mode and not-GNOME bars on screen, Devices › Details open
  (two clicks on its chevron did nothing; unverified whether that is the
  shared sandbox or a fault). Audit T1–T10 screenshots per finding are not
  assembled.
- Workspace-wide test run, clippy 1.99: only the touched crates were run.
