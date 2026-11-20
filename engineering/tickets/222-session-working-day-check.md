# Ticket 222 — A whole working day in the session, in a VM, on the released compositor

**Spec:** better-os-session.md S4 (flow 2), "Ready to become the default" item 2
**Blocked by:** 205, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 149, 156, 157, 158, 160, 174

## Why

Each surface is checked alone by its own ticket. A session is a desktop's
worth of small duties, and a missed one shows up as a dialog that never
appears (ADR 0038). This ticket runs the spec's flow 2 end to end, which is
also C8's gate: the gesture patch comes once the rest of the session works.

## What to build

- **Flow 2 of the spec**, step by step, in a disposable VM per base, from the
  built packages installed through Better Manager, with a replayed libinput
  recording for the four-finger workspace switch, a `hostapd` network, a USB
  drive passed through, an XEmbed program, Orca, a Better OS package update.
- Every step's result in Notes; every failure fixed in the ticket that owns
  the part, or, if no ticket owns it, a new ticket in this change.

## Done when

- [ ] Every step of flow 2 passes on Zorin OS 18 and Ubuntu 24.04, with
      screenshots and the journal in `/home/tim/Developer/better-os-evidence/222/`.
- [ ] No step depends on a terminal command.
- [ ] The spec's process table has no **unverified** mark left that this flow
      can settle.
