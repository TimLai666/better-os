# Ticket 262 — `better-lock`: the Better OS lock screen in the session

**Spec:** `design/login-screen-spec.md` sections 3 and 4; `engineering/specs/login-screen-security.md`; `better-os-session.md` (the "Screen lock" row of the process table)
**Blocked by:** 203, 260, 261, 263, 269

## Why

The session's locker is `cosmic-greeter` today (ticket 201). Under the
recommended answer to L1 the lock screen comes first, and it must look like
the login screen.

## What to build

- Binary `better-lock` (GPUI, on `login-screen-core`), started by the session
  launcher as its own child on logind's `Lock`, on the Lock action and on
  `cosmic-idle`'s idle lock; one per session.
- The wide and narrow layouts, quiet and entry (L3), the person's wallpaper,
  the status row, the power menu, the accessibility switches, the layout
  indicator, Caps Lock, the Zhuyin notice, the waits, the password change,
  notifications (L4) and media controls, with every string of the spec.
- PAM in-process under the new service `better-os-lock`
  (`/etc/pam.d/better-os-lock`, shipped in the session package), the secret
  path of the security review, `unlock_and_destroy` only after
  `authenticate` and `acct_mgmt` succeed or when logind sends the session
  `Unlock` (`loginctl unlock-session`, the text-console way back); logind's
  `LockedHint` set and cleared.
- Crash handling in the launcher: a restart within a second; after three
  crashes in 30 s, `cosmic-greeter`'s locker; a heartbeat file every 5 s
  under `$XDG_RUNTIME_DIR` and a restart after 30 s of silence;
  `better-os-session relock` (the launcher's subcommand from the security
  review), which starts the lock program again, refuses without a Better OS
  session of the account, and never unlocks.
- The sandbox of ticket 263 applied at start.
- The spec's section 4.5 states what the lock does not protect against (a
  program running as the person can unlock through logind, or take the
  compositor's lock after killing `better-lock`); this ticket adds no
  claim beyond it.
- `better-os-session.md`'s process table: the lock row names `better-lock`
  with `cosmic-greeter` as its fallback.

## Done when

- [ ] In the session VM (`packaging/tests/session-vm/`): lock, unlock with
      the password, a wrong password and the wait, Caps Lock, Zhuyin active
      when locking, notifications and media shown, the power menu, two
      outputs at scale 1 and 2; `kill -9` of `better-lock` brings it back
      locked; three kills fall back to `cosmic-greeter`; from a text console,
      `relock` brings the lock screen back and `loginctl unlock-session`
      unlocks; switching terminals never shows the session's content.
- [ ] logind's `LockedHint` follows the lock.
- [ ] Screenshots in light and dark, zh-TW and English, in
      `/home/tim/Developer/better-os-evidence/262/`, compared with the
      mockups in `design/login-screen/`.
