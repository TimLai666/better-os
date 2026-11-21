# Ticket 280 — `better-os-gnome-unlock`: a GNOME session locks without GDM

**Spec:** `design/login-screen-spec.md` section 5.6; `engineering/specs/login-screen-security.md` (the unlock service's limits); ADR 0044
**Blocked by:** the owner's decisions L1 (B or C) and L2 (a)

## Why

The owner's requirement (2026-11-21): every session can lock, without GDM.
GNOME Shell 46 locks only when `org.gnome.DisplayManager` answers, and
checks the password through GDM's reauthentication channel. Under the
Better OS greeter, Better OS answers instead, without patching the shell.

## What to build

- `better-os-gnome-unlock`, a root Rust service (zbus, its default flavor)
  in the `better-os-greeter` package, unit `better-os-gnome-unlock.service`
  (`Conflicts=gdm.service`, started only while the alias names the Better
  OS greeter, `Restart=always`, the review's sandbox):
  - `org.gnome.DisplayManager.Manager` at `/org/gnome/DisplayManager/Manager`:
    `Version` (the installed `libgdm1`'s version), `RegisterSession`
    (recorded), `OpenReauthenticationChannel` with every check of the
    security review, `OpenSession` and `RegisterDisplay` refused;
  - the private connection: one socket per request under
    `/run/better-os-gnome-unlock/`, one peer matched by `SO_PEERCRED`,
    `org.gnome.DisplayManager.UserVerifier` (`EnableExtensions`,
    `BeginVerification`, `BeginVerificationForUser`, `AnswerQuery`,
    `Cancel`, and every signal of GDM 46.2's `gdm-session.xml`) and
    `…UserVerifier.ChoiceList` at `/org/gnome/DisplayManager/Session`;
  - PAM for the three mapped services, `/etc/pam.d/better-os-gnome-unlock`
    (`@include common-auth`, `common-account`, `pam_faildelay`,
    `pam_gnome_keyring`), `-fingerprint`, `-smartcard`; answers in
    `login-screen-core`'s `Secret`.
- It refuses to start when `libgdm1`'s major version is not the one this
  ticket tested; the greeter (ticket 264) lists GNOME only while `Version`
  answers.
- Tests on a private system bus with a fake logind: every refusal of the
  review (another user, a remote session, a process that is not
  `gnome-shell`, a second peer on the socket, an unknown service name), a
  full conversation against a test PAM module, the secret test of ticket
  263.

## Done when

- [ ] In the session VM, under greetd with a test greeter, on Ubuntu 24.04
      and Zorin OS 18: GNOME shows Lock, locks on Super+L, on idle and on
      `loginctl lock-session`; unlocks with the password; refuses a wrong
      one; stays locked when the service is killed while locked and unlocks
      after it restarts; unlocks on `loginctl unlock-session` from a text
      console; the login keyring stays unlocked; switching to other
      terminals never shows the session's content.
- [ ] The same run repeated is part of the release check whenever
      `gnome-shell`, `libgdm1` or `gir1.2-gdm-1.0` changes version.
- [ ] Evidence in `/home/tim/Developer/better-os-evidence/280/`.
