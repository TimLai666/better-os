# Ticket 264 — `better-greeter`: the Better OS login screen on greetd

**Spec:** `design/login-screen-spec.md` sections 3, 5.2, 5.3, 5.5 to 5.8; `engineering/specs/login-screen-security.md`; ADR 0044
**Blocked by:** 260, 262, 263, 269, 279 (L1 is answered B and L7 is answered (a), 2026-11-22)

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
  only while `org.gnome.DisplayManager`'s `Version` answers, ticket 280, and
  checked again within 2 s right before `start_session`, with the spec's
  "GNOME can't lock right now" message when it does not answer), the
  layout indicator from locale1, the status row (NetworkManager and UPower,
  read only), the power menu through logind, the accessibility switches in
  `/var/lib/better-os-greeter/settings.json`, Better OS's artwork (ticket
  279), the heartbeat in `/run/better-os-greeter/alive` (written when
  connected to the compositor, `ready` once the first frame is presented on
  every output and the field has focus, plus `ready-seen` once per boot,
  then every 5 s), ticket 263's
  sandbox applied at start.
- greetd's IPC: framing, every request and response of `greetd-ipc(7)`,
  answering every prompt in order, `start_session` with the session's `Exec`
  and the environment the session needs (`XDG_SESSION_TYPE=wayland`,
  `XDG_SESSION_DESKTOP` from the file), exit within 5 s after success; the
  last-start record and the "didn't start" state.
- The greeter writes nothing to AccountsService: another account's record
  needs `org.freedesktop.accounts.user-administration` (`auth_admin_keep`).
  It passes `BETTER_OS_LOGIN_SESSION` and `BETTER_OS_LOGIN_LANGUAGE` in
  `start_session`'s environment, and `better-os-remember-login` (a small
  program and an XDG autostart entry in this package) writes them as the
  person, with `change-own-user-data`, once per login, in any session.
- Tests against a fake greetd socket: success, `auth_error`, several secret
  prompts, `info` and `error` messages, a password change, a broken reply,
  greetd vanishing.
- What `login-screen-core` (ticket 260) leaves to this program, from its
  security review of 2026-11-22:
  - write `login_screen_core::NOT_LOCKED_LOG_LINE` to the journal once when
    a field's `Secret::lock_warning()` says its pages are not locked;
  - make the field's buffer with `Secret::try_new()` (the only public
    constructor); when it fails, send the view model
    `Event::ServiceNotResponding` (「登入服務沒有回應。請重新開機。」, the
    power menu still works) and log the error, never end the greeter;
  - create `/run/better-os-greeter/` (owned by `_better-greeter`, 0700) so
    that it **survives a restart of `better-os-greeter.service`**: a
    `tmpfiles.d` entry, or `RuntimeDirectory=` with
    `RuntimeDirectoryPreserve=yes` (without it systemd deletes the
    directory when the unit stops, and every retry of the supervisor would
    end every wait); a container check restarts the unit and finds
    `attempts.json` still there;
  - load `Attempts` from `attempts::GREETER_RECORD` with
    `Attempts::load(path, boot_time_now())` into `Screen::login(…)
    .with_attempts(…)`, and save it whenever `Effects::attempts_changed`;
    log a record that cannot be read by its error (which names only the
    kind) and start empty;
  - pass `Screen::with_resolver(Box::new(attempts::NameService))` so a
    typed name counts under its account (`getpwnam_r`'s `pw_name`): the
    sandbox (ticket 263) must leave the name service reachable
    (`/etc/passwd`, `/etc/nsswitch.conf`, and `sssd`'s socket where it is
    used);
  - write the frame with `GreetdMessage::frame_bytes()` and wipe it, and add
    that socket writer to `READERS` in
    `crates/login-screen-core/tests/one_secret_reader.rs`, with its review;
  - pass `boot_time_now()` (`CLOCK_BOOTTIME`) as every event's time;
  - tag every greetd reply with the `ConversationId` of the
    `create_session` it answers (greetd replies in order, so the greeter
    counts its requests), and drop none itself: the view model drops stale
    ones;
  - read the session menu with `sessions::read_sessions(…,
    &app_catalog_platform::HostProbe::from_env())` so `TryExec` is honoured;
  - send `Event::Suspending` and `Event::Resumed` from logind's
    `PrepareForSleep(true / false)`.

## Done when

- [ ] Every state of the spec's section 3 renders in the session VM under
      greetd, in light and dark, zh-TW and English, matched against the
      mockups, with screenshots in
      `/home/tim/Developer/better-os-evidence/264/`.
- [ ] The fake-greetd tests pass, and ticket 263's secret test passes for
      this binary.
- [ ] After a login to GNOME and to the Better OS session, AccountsService
      holds the chosen session and language for that account, written by
      `better-os-remember-login`; a failed login writes nothing.
- [ ] Starting GNOME with the unlock service stopped is refused with the
      spec's message.

## Notes

From ticket 267 (the login supervisor, 2026-11-22), what this program must
do for the supervisor, which already reads it this way
(`crates/login-supervisor/src/read.rs`, `rules.rs`):

- `/run/better-os-greeter/alive`: a new file renamed into place each time,
  holding `starting` once the greeter runs under the compositor and `ready`
  once its first frame is presented on every output and the password field
  has focus, rewritten every 5 seconds while it runs. At most 64 bytes, a
  regular file. The supervisor judges it by change (a new inode or
  modification time), not by a time written in it, so it must really be
  rewritten every 5 s: 30 s without a change after `ready`, with no
  person's session, restarts the login screen.
- `/run/better-os-greeter/ready-seen`, created at the first `ready` of the
  boot: the backstop's first condition.
- `/run/better-os/greeter-mode`: `/usr/sbin/better-os-greeter` reads it at
  start; `safe` means the third attempt's safe mode (only the first card,
  software rendering if `cosmic-comp` has it, no fractional scale); absent
  or anything else is a plain start.
- The supervisor's stage 5 looks for a logind session of class `greeter`
  owned by `_better-greeter`, and its stage 9 for a person's session as a
  process under greetd of an account other than root and
  `_better-greeter`: the greeter's processes must all run as
  `_better-greeter`.
