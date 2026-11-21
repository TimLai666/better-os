# Ticket 264 — `better-greeter`: the Better OS login screen on greetd

**Spec:** `design/login-screen-spec.md` sections 3, 5.2, 5.3, 5.5 to 5.8; `engineering/specs/login-screen-security.md`; ADR 0044
**Blocked by:** 260, 262, 263, 269, 279, the owner's decision L1 (B or C), and L7

## Why

The login screen the owner asked for. It is built after the lock screen and
shares everything with it but the identity and the authentication path.

## What to build

- `/usr/sbin/better-os-greeter`: sets the greeter's environment and execs
  `cosmic-comp /usr/libexec/better-os/greeter/better-greeter`; starts the
  AT-SPI bus and Orca first when the screen reader switch is on.
- `better-greeter` (GPUI, on `login-screen-core`, `better_ui::layer`
  surfaces at the overlay layer with exclusive keyboard on every output):
  accounts from AccountsService, the chooser and "Other account…", the
  session menu from `/usr/share/wayland-sessions/` (a GNOME session listed
  only while `org.gnome.DisplayManager`'s `Version` answers, ticket 280), the
  layout indicator from locale1, the status row (NetworkManager and UPower,
  read only), the power menu through logind, the accessibility switches in
  `/var/lib/better-os-greeter/settings.json`, Better OS's artwork (ticket
  279), the heartbeat in `/run/better-os-greeter/alive` (written when
  connected to the compositor, `ready` once the first frame is presented on
  every output and the field has focus, then every 5 s), ticket 263's
  sandbox applied at start.
- greetd's IPC: framing, every request and response of `greetd-ipc(7)`,
  answering every prompt in order, `start_session` with the session's `Exec`
  and the environment the session needs (`XDG_SESSION_TYPE=wayland`,
  `XDG_SESSION_DESKTOP` from the file), exit within 5 s after success; the
  last-start record and the "didn't start" state.
- `SetSession` and `Language` written to AccountsService only after success.
- Tests against a fake greetd socket: success, `auth_error`, several secret
  prompts, `info` and `error` messages, a password change, a broken reply,
  greetd vanishing.

## Done when

- [ ] Every state of the spec's section 3 renders in the session VM under
      greetd, in light and dark, zh-TW and English, matched against the
      mockups, with screenshots in
      `/home/tim/Developer/better-os-evidence/264/`.
- [ ] The fake-greetd tests pass, and ticket 263's secret test passes for
      this binary.
