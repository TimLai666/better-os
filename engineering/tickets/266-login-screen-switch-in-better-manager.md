# Ticket 266 — Better Manager: use the Better OS login screen, and go back, in one action each

**Spec:** `design/login-screen-spec.md` section 6; ADR 0044 ("Switching on", "Switching off")
**Blocked by:** 265, 204, 205, the owner's decision L1 (B or C)

## Why

AGENTS.md: everything Better OS offers is complete after one action in
Better Manager, and undoing it is one action that leaves the machine as it
was.

## What to build

- `manager-daemon` methods, each with its own polkit action
  (`auth_admin_keep`): switch on (ADR 0044's five steps, capture first) and
  switch off (put back exactly, purge what the switch installed); the rules
  in a module with tests over a fake debconf, a fake file system and a fake
  `dpkg-reconfigure`.
- The Better OS session page's choice, its description, the confirmation,
  the next-boot state with Restart now, the on state with Go back, and the
  "GDM took over" notice (ticket 267's record), with every string of section
  6; the security review's way-back steps shown beside it.
- Removing the session switches the login screen off first.
- A health check: the chosen display manager is the recorded one.

## Done when

- [ ] In both VMs: on, restart, the Better OS login screen; off, restart,
      GDM; the four places compared byte for byte with the capture.
- [ ] Screenshots of the page in light and dark, zh-TW and English, in
      `/home/tim/Developer/better-os-evidence/266/`.
