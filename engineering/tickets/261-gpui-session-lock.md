# Ticket 261 — GPUI: a session lock window kind and a password field the input method leaves alone

**Spec:** `design/login-screen-spec.md` section 5.4
**Blocked by:** 257, the owner's decision L6

## Why

The lock program needs surfaces handed out by `ext_session_lock_v1`, one per
output. GPUI at the workspace's revision has `WindowKind::LayerShell` and no
session lock kind (`crates/gpui/src/platform.rs` lines 2000 to 2027 at
`f05b2a97`), and sets `ContentPurpose::Normal` on every text input
(`crates/gpui_linux/src/linux/wayland/client.rs` line 433), so a password
field is offered to the input method like any other.

## What to build

- In GPUI's Wayland backend (GPUI Fast after ticket 257): the session lock
  manager bound when the compositor offers it; an API to lock, to open one
  lock surface per output (sized by the compositor's configure, re-created
  when outputs change), to unlock and destroy, and the `locked` and
  `finished` events; an error like `LayerShellNotSupportedError` when the
  compositor has no session lock.
- A way for a text input to ask for the `password` content purpose and the
  `sensitive_data` hint, and for a window to keep text input disabled.
- Under L6 (a): offered to GPUI Fast as a pull request, carried in Better
  OS's fork until merged, under the amendment to ADR 0043 that L6 (a) needs
  (a new record, written in this ticket).
- `better_ui::layer` gains a `lock` module that opens a view on every
  output's lock surface, as `LayerSurfaces` does for layer surfaces.

## Done when

- [ ] On `cosmic-comp` in a headless sandbox (AGENTS.md's sandbox rules), a
      test program locks, shows a surface on each of two nested outputs at
      scale 1 and 2, receives keys only on the lock surfaces, and unlocks;
      killing it leaves the session locked, and a second instance can lock.
- [ ] A password field with the content purpose set is ignored by Fcitx 5
      with Chewing active (typed letters arrive as Latin), in the same
      sandbox.
- [ ] Screenshots in `/home/tim/Developer/better-os-evidence/261/`.
