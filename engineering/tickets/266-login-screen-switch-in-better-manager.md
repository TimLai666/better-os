# Ticket 266 — Better Manager: use the Better OS login screen, and go back, in one action each

**Spec:** `design/login-screen-spec.md` section 6; ADR 0044 ("Switching on", "Switching off", and the amendment of 2026-11-22)
**Blocked by:** 265, 268, 281, 204, 205

## Why

AGENTS.md: everything Better OS offers is complete after one action in
Better Manager, and undoing it is one action that leaves the machine as it
was. Neither direction may stop a running display manager: greetd ends the
person's session when it is stopped, and a GNOME session needs the unlock
service to unlock.

## What to build

- `manager-daemon` methods, each with its own polkit action
  (`auth_admin_keep`): switch on (ADR 0044's five steps, capture first,
  GDM's automatic login carried into greetd's `initial_session`, GDM's own
  file never edited) and switch off (the debconf value, the file and the
  link put back exactly; nothing stopped; effective at the next boot); the
  rules in a module with tests over a fake debconf, a fake file system and
  a fake `dpkg-reconfigure`.
- The purge of what the switch installed happens at Better Manager's first
  start after the boot that ran GDM again, as it finishes every pending
  put-back, and only when GDM is the running display manager.
- The Better OS session page's choice, its description, the confirmation
  (with the automatic-login line when it applies), the next-boot states in
  both directions with Restart now, the on state with Go back, and the
  "fell back to text sign-in" and "had to be restarted" notices (ticket
  267's records), with every string of section 6; the security review's
  way-back steps shown beside it.
- Removing the session switches the login screen off first; the session's
  removal waits for the same boot.
- A health check: the chosen display manager is the recorded one, and
  `_better-greeter` exists and is usable (repaired here when it is not,
  because the supervisor does not touch `/etc` at boot).

## Done when

- [ ] In both VMs: on, restart, the Better OS login screen; off while logged
      in through it (the session keeps running and, for GNOME, still
      unlocks), restart, GDM; the purge done at the next start; the four
      places compared byte for byte with the capture.
- [ ] With GDM's automatic login on: on, restart, the same account logged in
      automatically; off, restart, GDM logs it in automatically.
- [ ] Screenshots of the page in light and dark, zh-TW and English, in
      `/home/tim/Developer/better-os-evidence/266/`.
