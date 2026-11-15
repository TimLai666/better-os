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
- Removal: the better-touchpad package gets a postrm
  (`packaging/maintainer/autostart-overrides.sh`, written by `build-deb.sh`,
  asserted by `verify-deb.sh`) that deletes, for every account in
  `/etc/passwd` in the login range (`UID_MIN..UID_MAX` from `/etc/login.defs`, default 1000..60000) with an absolute home other than `/`, only a regular file in
  `~/.config/autostart/{touchegg,fusuma}.desktop` owned by that account and
  carrying the exact marker line, and never through a symlinked `~/.config` or `~/.config/autostart`. Shell test
  `packaging/tests/autostart-overrides.test.sh` (in CI); a Rust test keeps the
  marker in `competitors.rs` and `build-deb.sh` the same. An override under a
  custom `XDG_CONFIG_HOME` is invisible to a maintainer script and stays; the
  page's "Turn back on" still removes it.
- Scroll speed: tested on a private GNOME Shell 46.0; an extension cannot see
  a client's scroll at all. Evidence and the alternative's estimate are in
  ADR 0031, superseded by ADR 0030: the owner decided it is built, in
  ticket 125. The Vertical and Horizontal rows stay unavailable on GNOME 46
  here until ticket 125 lands.

Screenshots (private headless GNOME Shell 46, in-memory backend, demo mode):
every sub-page, zh/en, light/dark, 1600 and 960 (48), Devices › Details and
the gesture preview for each of those 8 combinations, safe mode and not-GNOME
bars, Home with "safe mode on" and "gestures turned themselves off".

Audit findings, each against the screenshots above (the audit's own list is
only this ticket's one-line summary, and it groups T5–T10):

- T1 mock notice: no mock or adapter name anywhere (`*-pointer`, `*-diagnostics`);
  the only banner is demo mode's own.
- T2 English inside Chinese: `zh-*` shots of every sub-page, including
  gesture names, problems, and unavailable reasons.
- T3 "requested / effective": `zh-*-pointer` reads 你選的 · 目前生效,
  `en-*-pointer` "You chose · In effect".
- T4 identifiers and an English health word on the overview: "This
  touchpad" shows a name and a word (`*-pointer`); identifiers only under
  Devices › Details (`*-devices-details`).
- T5–T10 hard-coded dark theme and misaligned controls: every sub-page in
  light and dark at 1600 and 960 (`{zh,en}-{light,dark}-{1600,960}-*`), with
  controls on Fluent cards right-aligned in one column.

Install and uninstall, clicked through in the same private GNOME Shell 46,
demo mode (`install/01`–`10`): not installed, Apps › Better Touchpad,
installing, installed and healthy, the Touchpad page working, Manage, the
remove confirmation with "Also delete my settings and history" ticked by
default, removing, removed, and the Touchpad page back to "needs to be
installed". Demo mode runs the mock driver, so these screens show the flow,
not the extension or settings work; that is shown by `tests/touchpad_install.rs`
and by `tests/gsettings_session.rs` run against the private shell's own dconf,
where the shell moved the extension from initialized (6) through enabled to
disabled (2) as the controller wrote.

Install-time setup (after ticket 124 merged):

- The extension: `manager_platform::shell_extensions` adds
  `touchpad-adapter@betteros.org` to the person's `org.gnome.shell
  enabled-extensions` (and takes it out of `disabled-extensions`) on install,
  enable, update, restore, and verify, and takes it out on disable and
  removal, leaving every other entry alone. Found from dpkg's file list like
  units; the health stage fails `shell_extension.not_enabled` when it is off;
  the startup pass turns an enabled component's extension back on. `gsettings
  set` exits 0 when dconf refuses, so every write is read back. A running
  shell loads it at the next login. Not the reviewed-defaults engine: that
  engine is driven only by the Defaults page and command, never by
  manager-core, and a new integration kind would make every released manager
  refuse the fetched better-touchpad manifest (closed set).
- The default gesture profile: the gesture service's first run (no gesture
  file at all) writes the Mac-style preset, capturing the empty configuration
  first, so Restore goes back to no Better OS gesture. A file the person
  saved, including all-off, is never replaced. This replaces Issue #3's
  "installation changes no gesture" by the owner's rule of 2026-11-13. It is
  in the service rather than manager-core so the manager does not write a
  component's own data file.
- Removal puts the touchpad settings back: the package ships
  `/usr/lib/better-os/before-removal/better-touchpad`
  (`touchpad-platform`'s `better-touchpad-before-removal`), which writes the
  captured values back through the GNOME backend; manager-core finds it in
  dpkg's file list and runs it as the person before anything else in the
  removal, and a failure stops the removal with nothing changed
  (`before_removal.failed`). Better Touchpad writes no keybinding, and GNOME's
  own swipe gestures come back when the extension is turned off.
- Personal data: `.config/better-os/touchpad` was already on ticket 124's
  allowlist and is everything the component keeps.

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

- Scroll speed moved to ticket 125 (ADR 0030). Note for it: holding a scale factor in the GJS adapter is configuration,
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
