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

## Progress (2026-11-15, second pass)

Built and tested since the first pass (all of the first pass's list stands):

- Competing gesture tools: `pages/touchpad/competitors.rs` finds X11 Gestures
  (shell `GetExtensionInfo`), Touchégg (the client process and its autostart
  entry; the root daemon is ignored), and fusuma (by command line), for this
  account only, read-only. "Turn off" disables the extension through GNOME
  Shell, or hides the autostart entry with a marked `Hidden=true` file in
  `~/.config/autostart`; "Turn back on" removes only that file, and a file the
  person wrote is never rewritten. Nothing is killed: a running copy stops at
  the next sign-in, and the page says so. Unit tests over a scratch `/proc`
  and autostart tree; shown working against a real GNOME Shell 46 with a
  stand-in extension and an autostart entry (screenshots `competitors-*`).
- Health checks named in words; the technical column is labelled as such.
- "Read again" moved onto the "This touchpad" card so the selector bar fits
  at 960 px in English.
- Devices › Details: the chevron works (screenshot `*-devices-details`); the
  earlier failure was the shared wayhand sandbox, where another worker's
  windows took the clicks.
- Scroll speed: tested on a private GNOME Shell 46.0; an extension cannot see
  a client's scroll at all. Evidence and the alternative's estimate are in
  ADR 0028 (proposed, owner decision). Nothing built.

Screenshots (private headless GNOME Shell 46, in-memory backend, demo mode):
every sub-page, zh/en, light/dark, 1600 and 960 (48), Devices › Details and
the gesture preview for each of those 8 combinations, safe mode and not-GNOME
bars, Home with "safe mode on" and "gestures turned themselves off".

Still open:

- Install-time setup (enable the shell extension, apply a default gesture
  profile at install): waits for ticket 124 to merge, by the coordinator's
  instruction.
- Scroll speed: owner decision on ADR 0028.
- Uninstalling Better Touchpad does not yet remove an autostart override it
  wrote for a competing tool; that belongs with the install-time work above.

Checks run: fmt; clippy `+stable` and `+1.99.0`, workspace, all targets,
`-D warnings` clean; `cargo +1.95 check` of the touched crates; license
inventory current; workspace tests: 270 test binaries pass, 43 tests fail in
crates this ticket does not touch (app-catalog-platform, awake-platform,
better-core, files-gui, files-platform, launcher-platform), every one with
"Too many open files": the machine's inotify instance limit (128) was used up
by other sessions during the run.

## Progress (2026-11-15, first pass)

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
