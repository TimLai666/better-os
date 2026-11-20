# Ticket 228 — The Better OS session becomes the default

**Spec:** better-os-session.md S8, "Ready to become the default"
**Blocked by:** 205, 222, 224, 227, the surfaces the bar lists (149, 152, 156–158, 160, 213, 210, 211, 212, 215, 216, 214, 144), and the owner accepting ADR 0040

## Why

Until the bar is met, GNOME stays installed and the default (ADR 0038, C1).
Once it is, "default" has to mean something checkable.

## What to build

- **The record**: each of the seven items of "Ready to become the default"
  with its evidence, in Notes, and the owner's go-ahead.
- **AGENTS.md** as ADR 0040 accepted it.
- **What default means**, as the owner confirms here: Better Manager's Home
  offers the session first to a person on GNOME (one card, one action);
  `install.sh` on a fresh 24.04 machine installs the session with
  `better-manager` and makes it the next login's, saying so, with GNOME left
  installed; the README's description of Better OS.
- GNOME stays supported: its tickets (151, 159, 161's GNOME build) get a
  dated line saying whether they are now built, kept waiting, or withdrawn by
  the owner's answer.

## Done when

- [ ] The seven items recorded with evidence.
- [ ] Home's card and `install.sh`'s step tested (unit tests for the card's
      states; `install.sh`'s fixture tests for 22.04, which gets no session).
- [ ] AGENTS.md changed as ADR 0040 says.
