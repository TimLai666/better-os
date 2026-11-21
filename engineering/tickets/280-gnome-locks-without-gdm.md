# Ticket 280 — `better-os-gnome-unlock`: a GNOME session locks without GDM

**Spec:** `design/login-screen-spec.md` section 5.6; `engineering/specs/login-screen-security.md` (the GNOME unlock service); ADR 0044 and its amendment of 2026-11-22
**Blocked by:** none (the owner answered L1 B and L2 a on 2026-11-21)

## Why

The owner's requirement (2026-11-21): every session can lock, without GDM.
GNOME Shell 46 locks only when `org.gnome.DisplayManager` answers when the
shell starts, and checks the password through GDM's reauthentication
channel. Under the Better OS login screen, Better OS answers instead,
without patching the shell.

## What to build

- `better-os-gnome-unlock`, a root Rust service (zbus, its default flavor)
  in the `better-os-greeter` package; unit `better-os-gnome-unlock.service`
  (`Type=dbus`, `BusName=org.gnome.DisplayManager`, `Conflicts=gdm.service`,
  `Restart=always`, the review's sandbox) and its own bus policy file:
  - `org.gnome.DisplayManager.Manager` at `/org/gnome/DisplayManager/Manager`:
    `Version` (the installed `libgdm1`'s version), `RegisterSession`
    (recorded), `OpenReauthenticationChannel` answered only for the caller's
    own account from a local session on a seat (from
    `GetConnectionCredentials`; the executable check is a filter, not a
    boundary), `OpenSession` and `RegisterDisplay` refused;
    `LocalDisplayFactory` calls answered with an error;
  - the private connection as GDM does it: `/run/better-os-gnome-unlock/`
    0711, one socket per request at 0666, one peer whose `SO_PEERCRED` and
    `EXTERNAL` uid and pid match the request, then the socket removed;
    `org.gnome.DisplayManager.UserVerifier` (`EnableExtensions`,
    `BeginVerification`, `BeginVerificationForUser`, `AnswerQuery`,
    `Cancel`, and every signal of GDM 46.2's `gdm-session.xml`) and
    `…UserVerifier.ChoiceList` at `/org/gnome/DisplayManager/Session`;
  - PAM for the three mapped services, `/etc/pam.d/better-os-gnome-unlock`
    (`@include common-auth`, `common-account`, `pam_faildelay`,
    `pam_gnome_keyring`), `-fingerprint`, `-smartcard`: `authenticate`,
    `acct_mgmt`, `setcred(PAM_REINITIALIZE_CRED)`, with the session's
    `XDG_RUNTIME_DIR` and session bus address in PAM's environment; each
    conversation on its own thread with a 60 s limit; at most one channel
    per session; per-uid backoff after five failures (1, 2, 4, 8, 30 s);
    answers in `login-screen-core`'s `Secret`.
- A `libgdm1` major version it was not tested with: it keeps serving GNOME
  sessions already running and answers `Version` with an error only for
  sessions that start afterwards.
- Tests on a private system bus with a fake logind: every refusal of the
  review (another account, a remote session, a session not on a seat, a
  second peer, a peer with the wrong pid, an unknown service name), a full
  conversation against a test PAM module, a hung test PAM module that stalls
  only its own conversation, the backoff, the secret test of ticket 263.

## Done when

- [ ] In the session VM, under greetd with a test greeter, on Ubuntu 24.04
      and Zorin OS 18: GNOME shows Lock, locks on Super+L, on idle and on
      `loginctl lock-session`; unlocks with the password; refuses a wrong
      one; stays locked when the service is killed while locked and unlocks
      after it restarts; unlocks on `loginctl unlock-session` from a text
      console; switching to other terminals never shows the session's
      content.
- [ ] GNOME started while the service is stopped, and while it is stopped
      with `SIGSTOP`: the greeter refuses to start it (ticket 264); started
      by hand anyway, the result (no screen shield, a 25-second start) is
      recorded.
- [ ] Investigated and recorded with evidence: the login keyring after an
      unlock (`pam_gnome_keyring` run by a root service); a hung
      `pam_sss`-like module while a second GNOME session unlocks; what
      GNOME's "Switch user" button on the lock screen shows and does
      (`unlockDialog.js` lines 838 to 862: `Gdm.goto_login_session_sync`),
      and, if it is shown, the button hidden through
      `org.gnome.desktop.screensaver user-switch-enabled` set system-wide
      while the Better OS login screen is chosen.
- [ ] A `libgdm1` version change while a GNOME session is locked: the
      session still unlocks.
- [ ] The same run repeated is part of the release check whenever
      `gnome-shell`, `libgdm1` or `gir1.2-gdm-1.0` changes version.
- [ ] Evidence in `/home/tim/Developer/better-os-evidence/280/`.
