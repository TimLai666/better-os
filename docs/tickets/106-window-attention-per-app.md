# Ticket 106 — The person decides, per app, what happens when a window asks for attention

**Blocked by:** 84 (done) · **Status:** ready

## Why

On the owner's Zorin 18.1 (GNOME Shell 46.0, Wayland), Claude Desktop
5.11492.2 asks for attention every time its window loses focus. GNOME turns
each request into a "「Claude」視窗已就緒" banner (`“%s” is ready`,
`js/ui/windowAttentionHandler.js` in `libshell-14.so`, connected to
`window-demands-attention` and `window-marked-urgent`), and the Zorin
taskbar marks the app's icon. The banner follows the app's notification
policy (`MessageTray.NotificationPolicy.newForApp`), so turning off the
app's banners in GNOME Settings hides it, but nothing controls the rest.

The owner, 2026-11-14, asked for a Better Manager switch for this, set
separately for each app.

## What to build

- **Four choices per app**, on a "Window attention" section of the control
  center's System page (ticket 86) or its own page if ticket 86 is not
  merged yet:
  - **Banner and taskbar mark** (GNOME's behaviour today).
  - **Taskbar mark only** (no banner, no entry in the notification list).
  - **Nothing** (the request is ignored).
  - **Bring the window forward** (the window is activated, as if the
    person had clicked it; for apps the person trusts).
- **A default for every app**, set to **Taskbar mark only**, with a list of
  apps that have their own choice. An app appears in the list after it has
  asked for attention once (Better OS records the app id and name), and can
  also be added by hand from the installed applications.
- **Done in Better OS's GNOME Shell extension** (within ADR 0012 and 0015):
  it replaces the handler on `global.display` for `window-demands-attention`
  and `window-marked-urgent`, looks up the window's app id
  (`Shell.WindowTracker`), and applies the choice. "Taskbar mark only" keeps
  the window's urgency so the taskbar still shows it, and clears it when the
  window is focused. The extension reads the choices from its GSettings
  schema; Better Manager writes them. Disabling the extension or
  uninstalling restores GNOME's handler exactly.
- Better Manager shows the setting only when the extension is enabled, and
  offers to enable it otherwise.

## Acceptance

- [ ] In a nested or headless GNOME Shell 46 with the extension (ticket 102's
      headless method), a test window that demands attention produces, for
      each of the four choices, exactly the described result; screenshots.
- [ ] A per-app choice overrides the default; removing it falls back.
- [ ] Disabling the extension restores the stock banner.
- [ ] Tests never change the person's real GNOME settings.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      the extension's checks, and the license check pass.
