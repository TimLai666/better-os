# Ticket 243 — Accounts in Better Manager

**Spec:** `design/system-settings-spec.md` section 13 (Accounts); ADR 0042 (proposed)
**Blocked by:** 231, the owner's decision M6, and the owner's decision M1

## Why

Users and passwords are COSMIC Settings' in the session and GNOME Settings'
under GNOME, both over AccountsService.

## What to build

- Spec section first, then Accounts: your name and picture
  (`change-own-user-data`), your password, other users (add, remove,
  administrator), automatic sign-in; all over AccountsService under ADR 0042.
- Decide in the spec, with a security note, how a standard (non-administrator)
  account changes its own password, since `change-own-password` is
  `auth_admin` on Ubuntu (GNOME Settings runs `passwd` for it); bring the
  choice to the owner if it needs anything outside ADR 0042.

## Done when

- [ ] Tests against a fake AccountsService for every polkit outcome and a
      user removed elsewhere.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/243/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
