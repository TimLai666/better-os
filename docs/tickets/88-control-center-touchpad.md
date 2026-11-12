# Ticket 88 — Control center: Touchpad

**Blocked by:** 84 · **Status:** ready

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
