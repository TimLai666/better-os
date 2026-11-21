# Ticket 278 — Fingerprint and smart card at the lock and login screens

**Spec:** `design/login-screen-spec.md` section 3.10
**Blocked by:** 262, 264, 280

## Why

The layout reserves a line for them. How each can work differs between the
two programs, because greetd runs one PAM conversation at a time.

## What to build

- The lock screen: a second PAM conversation (`better-os-lock-fingerprint`,
  `pam_fprintd`) running beside the password one, either succeeding unlocks;
  the hint in the reserved line; the same for a smart card (`pam_pkcs11` or
  `pam_sss`).
- The login screen: what greetd 0.9.0 and later allow (one service, its
  modules in order), measured, and the spec's copy adjusted to what is
  possible.
- GNOME's lock: the unlock service's `gdm-fingerprint` and `gdm-smartcard`
  conversations, which GNOME Shell runs beside the password one.

## Done when

- [ ] A fingerprint unlock in the VM with a simulated `fprintd` device, and
      the findings for the login screen written into the spec.
