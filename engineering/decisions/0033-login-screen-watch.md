# ADR 0033: A login screen watch restarts a hung GDM once per boot

## Status

Accepted on 2026-11-16 by the owner (ticket 140). Adds a system unit to the
privileged package; ADR 0007's protocol and ADR 0029's sysfs rule are
unchanged.

## Context

Ticket 139: on the owner's machine GDM sometimes starts and never opens its
greeter session. The boot stays on the splash until a forced power-off (once
for 16 minutes). Every good boot opens the greeter about 4 seconds after
`gdm.service` starts. The cause is not known yet, and the owner asked for two
things in the meantime: the login screen should come back on its own, and
Better OS should know and say that it happened, so nobody has to notice it
and read the journal.

## Decision

- **Where it runs.** `better-greeter-watch.service`, a system unit shipped in
  `better-manager-daemon`, the one Better OS package allowed to install system
  units and act as root. It runs
  `/usr/libexec/better-manager-daemon --watch-login-screen`, a mode of the
  daemon's binary like `--restore-hardware`, not a second program: the rules
  live in `manager_daemon::boot_watch` beside the daemon's other privileged
  code, and the records' format in `manager_ipc::boot_watch`, which both the
  watch and Better Manager read.
- **Never on the boot's path.** `Type=oneshot`, `TimeoutStartSec=180`,
  `WantedBy=graphical.target` with `DefaultDependencies=no`, so the target
  gains no `After=` on it and nothing waits for it. It is not ordered after
  `gdm.service` either: a GDM start job that hangs must not hold the watch's
  own start. The program polls `display-manager.service` instead, and gives
  up after 60 s if GDM never becomes active (systemd reports a display
  manager that fails to start; this watch is for one that starts and then
  shows nothing).
- **Automatic login.** When any of `/etc/gdm3/custom.conf`,
  `/etc/gdm3/daemon.conf`, `/etc/gdm/custom.conf`, or `/etc/gdm/daemon.conf`
  turns on `AutomaticLoginEnable` in a `[daemon]` section, no login screen
  is expected and GDM is never restarted on that machine. Timed login still
  shows the login screen, so it is watched, with the deadline moved to
  `TimedLoginDelay` (GDM's default 30 s) plus 30 s.
- **The rule.** Once GDM is active, logind is asked every second, read-only,
  over D-Bus. A session of class `user*` or `lock-screen` anywhere means a
  person is there, and the boot is left alone. GDM's greeter, or any
  `wayland`/`x11`/`mir` session on `seat0`, is the login screen. If neither is
  there 30 s after GDM became active:
  1. the evidence is read (GDM's and logind's journal for this boot, kernel
     lines naming `ucsi` for ticket 101, `systemctl --failed`, seat0's
     `CanGraphical`, the entries of `/sys/class/drm`, the session list);
  2. the event record is written, outcome `pending`, before anything is
     restarted, so a watch killed later still marks the boot;
  3. logind is asked once more, and a person who logged in on a text console
     meanwhile keeps GDM; so does a `gdm-session-worker` in GDM's own cgroup
     whose process title names a person's login (`pam/gdm-password`,
     `gdm-autologin`, `gdm-fingerprint`, `gdm-smartcard`). The worker that
     launches the greeter (`gdm-launch-environment`) does not count: a
     greeter that hangs after it started is the case the watch is for;
  4. `systemctl restart --no-block gdm.service` (or `gdm3.service`, the only
     two units the watch will name);
  5. it waits up to 30 s for the login screen and records the outcome.
- **At most once per boot.** The unit has `RemainAfterExit=yes`, so a second
  `systemctl start` runs nothing. Every run writes `last-boot.json` with the
  boot id, whatever it found, and an event record's name carries the boot id;
  a boot named by either is never watched again, even when the unit is
  restarted by hand. Without a readable boot id nothing is done. logind
  errors other than a session that closed while being read (`UnknownObject`,
  `NoSuchSession`) stop the watch with nothing restarted.
- **No GDM.** When `display-manager.service` is not found, or resolves to
  another display manager, the watch does nothing and says so in
  `last-boot.json`.
- **Records, and who reads them.** `/var/lib/better-os/boot-watch/`
  (`StateDirectory=`, 0755) holds what anyone may read, 0644:
  `settings.json`, `last-boot.json` (what the watch did on the latest boot),
  and `summary.json` (every record without its evidence). The evidence,
  `evidence/<unix seconds>-<boot id>.json` per late login screen, is 0640 in a
  0750 directory, both root:adm. The journal excerpts can carry what any
  system journal carries (user names in logind's lines, device serials), and
  `adm` is the group Ubuntu already trusts with the system journal: the first
  user of an install is in it, a second account usually is not. So the person
  who administers the machine sees everything, and anyone else sees what
  happened and is told the details need an administrator. Each journal
  section keeps its last 64 KiB, whole lines, and nothing is redacted from
  them; sessions are kept by id, class, type, and seat, not by user name. The
  newest 20 events are kept, and this boot's own record is never the one
  pruned, whatever its clock named it. Every file is written to a temporary
  created new with `O_NOFOLLOW` and renamed into place, so a planted link
  is never written through.
- **Interrupted.** A record still `pending` from an earlier boot is shown as
  interrupted: the restart was asked for and the watch never wrote how it
  ended.
- **What a person sees.** Home shows "The login screen didn't appear at
  startup on {date}" with what Better OS did, for a week after it happened,
  as a warning when the login screen came back and an error when it did not.
  Health's card lists every kept record with its evidence line and the
  records folder. `better-manager-cli doctor` prints the same.
- **Off switch.** `settings.json` in the same directory, `{"enabled": false}`
  when off. It is written only by the privileged service, through
  `org.betteros.BootWatch1.SetEnabled` and the polkit action
  `org.betteros.manager.login-screen-watch` (`auth_admin_keep`). The window
  asks; it never writes. An unreadable file means the watch does nothing and
  Health says the setting could not be read.
- **Install and uninstall.** The postinst enables the unit on a first install
  and, on an upgrade, only when it was enabled (debhelper's `was-enabled`
  shape), so an administrator's `systemctl disable` survives upgrades. The
  prerm disables it on removal and the postrm removes the whole directory,
  records and setting, on removal and on purge.
- **The setting goes with the package.** AGENTS.md's "Also delete my
  settings and history" box belongs to a component Better Manager
  uninstalls. `better-manager-daemon` is not one: it has no manifest, Better
  Manager never offers to remove it, and it leaves only through apt, which
  shows no box. Its on/off choice is therefore removed with it, as the charge
  limit's stored choice is on purge. A reinstall starts with the watch on.

## Consequences

- The watch adds no time to a good boot: nothing waits for it, and it exits a
  few seconds after the greeter appears.
- A hang that lasts less than 30 s is not touched; one that lasts longer costs
  the person 30 s plus GDM's restart, instead of a forced power-off.
- The watch treats the symptom. Ticket 139 still owns the cause; the records
  are its evidence.
- `packaging/boot-watch/run.sh` is the end-to-end check, in a container with
  systemd as PID 1 and a stand-in `gdm.service` that never opens a session
  (CI's `package` job, because this machine has no Docker): it proves the
  restart, the record, its mode, that graphical.target is not ordered after
  the unit, and that a second run on the same boot restarts nothing.

## Not chosen

- **Restarting more than once, or rebooting.** A GDM that hangs again after a
  restart has a cause a second restart will not fix, and a reboot loop is
  worse than a stuck splash.
- **Ordering after `gdm.service` or `Before=graphical.target`.** The first
  ties the watch to the job it is watching; the second makes every boot wait
  for it.
- **A user-session or GNOME component.** There is no user session to run in;
  that is the failure.
- **A setting file written by the window.** The directory is root's, and the
  window does not cross the privileged boundary (AGENTS.md).
