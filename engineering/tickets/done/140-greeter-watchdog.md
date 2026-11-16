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

- [x] Tests: no greeter → evidence recorded, one restart, outcome recorded; a
  greeter in time → nothing; a session already present → nothing; second
  failure → no second restart; record retention.
  (`manager_daemon::boot_watch` tests, on a fake logind, systemd, journal,
  and clock.)
- [x] A VM or container check where GDM is made to hang (a stand-in display
  manager that never opens a session) shows the restart and the record.
  `packaging/boot-watch/run.sh` in CI's `package` job. CI-only: this
  machine has no Docker daemon and no VM image, and none was downloaded.
- [x] The Home/Health card seen in a sandbox (headless GNOME Shell 46,
  English and zh-TW, light and dark, real clicks; the switch written through
  the daemon's BootWatch1 on a private bus).

## What was built

- **The decision** is ADR 0033. The watch is
  `better-manager-daemon --watch-login-screen`, run by
  `better-greeter-watch.service` (`Type=oneshot`, `TimeoutStartSec=180`,
  `DefaultDependencies=no`, `WantedBy=graphical.target`, so nothing in the
  boot waits for it). It polls `display-manager.service` for GDM (60 s), then
  logind every second until 30 s after GDM became active. A person's session
  anywhere leaves the boot alone; GDM's greeter or a graphical session on
  seat0 is the login screen.
- **Late:** the evidence (GDM's and logind's journal tails, kernel `ucsi`
  lines, `systemctl --failed`, seat0 `CanGraphical`, `/sys/class/drm`, the
  session list) is written as a `pending` record first, logind is asked
  again, then `systemctl restart --no-block gdm.service`, and the outcome
  after up to 30 s. A boot with a record is never restarted again; no boot id
  means nothing is done. No GDM, another display manager, a GDM that never
  starts, or the setting off: nothing, with the reason in `last-boot.json`.
- **Records** under `/var/lib/better-os/boot-watch/` (0755, files 0644), the
  newest 20 kept, 64 KiB per journal section. Format and reader in
  `manager_ipc::boot_watch`; what a person is told in
  `manager_core::login_watch`.
- **Better Manager:** Home's attention card for a late login screen in the
  last 7 days (a warning when it came back, an error when it did not), and
  Health's Login screen watch expander with the switch, this startup's
  result, every record with its evidence line, and the folder.
  `better-manager-cli doctor` prints the same in English with UTC times.
- **Off switch:** `org.betteros.BootWatch1.SetEnabled` behind
  `org.betteros.manager.login-screen-watch`, writing `settings.json`. The
  window only asks.
- **Package:** the unit ships in `better-manager-daemon`, the postinst enables
  it (never starts it), the prerm disables it on removal, the postrm removes
  the directory on removal and purge. `verify-deb.sh` checks all of it.

## Not done here

- No real boot has been watched and no real GDM restarted; the container
  check is the closest evidence, and it cannot open a greeter session, so
  "the login screen appeared in time" is proved by fakes only.
- The cause of the hang is still ticket 139's.
