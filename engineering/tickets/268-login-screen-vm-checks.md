# Ticket 268 — The login and lock screens on Ubuntu 24.04 and Zorin OS 18, and the way back done by hand

**Spec:** `design/login-screen-spec.md` section 7; `engineering/specs/login-screen-security.md` ("Not proven"); ADR 0044
**Blocked by:** 266, 267

## Why

ADR 0044 makes these checks the condition for offering the greeter.

## What to build

- In `packaging/tests/session-vm/`, on both bases: `cosmic-comp` as the
  greeter's compositor under `_better-greeter` (and whether the account
  needs the `video` group); greetd starting the Better OS session and GNOME;
  GNOME's missing lock under greetd confirmed or refuted; how long the black
  screen between the greeter and the session lasts; five greeter failures
  bringing GDM; a `SIGSTOP`ped greeter replaced by GDM; a layer surface of a
  session program tried over the lock screen; AccessKit and Orca in the
  greeter.
- The security review's "way back to GDM" steps performed by hand from a
  text console, every screen captured; and "getting back into a locked
  session".
- The results written into the spec's section 7 and the review's "Not
  proven".

## Done when

- [ ] Every item above has a result and evidence in
      `/home/tim/Developer/better-os-evidence/268/`.
