# Ticket 140 — A watchdog brings the login screen back when GDM hangs, and says so

**Blocked by:** none (complements ticket 139, which finds the cause)

## Why

GDM sometimes starts and never opens the login screen (ticket 139: once for
16 minutes until a forced power-off). The owner asked on 2026-11-16 for a
watchdog, and that Better OS itself must know when it happened rather than
the owner having to notice and report it.

## What to build

- **Detect**: a system unit shipped in `better-manager-daemon` (system units
  belong to the privileged package; record the design in an ADR) that runs
  once per boot after `gdm.service` starts and asks logind (D-Bus, read-only)
  whether a greeter session of the `gdm` user, or any graphical session on
  seat0, exists. Deadline: 30 s after GDM started (normal is about 4 s).
- **Act**: if there is none, record the evidence first (GDM's and logind's
  journal for this boot, seat0's `CanGraphical`, DRM devices present, failed
  units, the UCSI messages of ticket 101), then restart `gdm.service` once,
  wait again, and record the outcome. Never more than one restart per boot;
  never touch a boot where a person's session already exists.
- **Know**: each event is a record under `/var/lib/better-os/boot-watch/`
  (readable by the person; at most the last 20 kept). Better Manager's Home
  and Health show "The login screen didn't appear at startup on <date>; Better
  OS restarted it" with the details, and `better-manager-cli doctor` lists it,
  so it is visible without anyone looking at logs.
- **Off switch and uninstall**: a setting in Better Manager turns the
  watchdog off; uninstalling removes the unit and its records.

## Done when

- Tests: no greeter → evidence recorded, one restart, outcome recorded; a
  greeter in time → nothing; a session already present → nothing; second
  failure → no second restart; record retention.
- A VM or container check where GDM is made to hang (a stand-in display
  manager that never opens a session) shows the restart and the record.
- The Home/Health card seen in a sandbox.
