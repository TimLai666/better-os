# Ticket 265 — The `better-os-greeter` package: the login path's units, configuration, account, bus policy, and display-manager registration

**Spec:** `design/login-screen-spec.md` sections 5.5 and 5.10 ("The units, and what starts what"); `engineering/specs/login-screen-security.md`; ADR 0044 and its amendment of 2026-11-22
**Blocked by:** 264, 267, 280, 203

## Why

The login path has to be a display manager Debian's own switch knows, so
that `gdm3` upgrades keep the choice and `sudo dpkg-reconfigure gdm3` takes
it back; and its units have to be wired so that no retry, fall back,
switch or upgrade stops the wrong thing.

## What to build

- `better-os-login-supervisor.service` (ticket 267): the unit
  `display-manager.service` aliases; `Type=notify`, `WatchdogSec=10`,
  `Restart=always`, `RestartSec=1`, `StartLimitIntervalSec=60`,
  `StartLimitBurst=5`, `OnFailure=better-os-login-backstop.service`,
  `Wants=better-os-gnome-unlock.service better-os-login-backstop.timer`;
  no `PartOf=`, `BindsTo=` or `Requires=` on any unit it controls; the
  security review's sandbox (`IPAddressDeny=any`, never `PrivateNetwork=`).
- `better-os-greeter.service`, written for Better OS, not copied from
  Debian's `greetd.service` (which uses VT 7, `TimeoutStopSec=30s`,
  `Restart=always` and `After=… plymouth-quit-wait.service`):
  `ExecStart=greetd --config /etc/greetd/better-os-greeter.toml`,
  `Conflicts=getty@tty1.service`, `After=better-os-gnome-unlock.service`,
  `KeyringMode=shared`, `Restart=no`, `TimeoutStopSec=4`, `SendSIGKILL=yes`,
  `OnFailure=plymouth-quit.service` only; no `[Install]`; started only by
  the supervisor.
- `better-os-gnome-unlock.service` (ticket 280): `Type=dbus`,
  `BusName=org.gnome.DisplayManager`, `Restart=always`,
  `Conflicts=gdm.service`; and its own bus policy file
  (`/usr/share/dbus-1/system.d/better-os-gnome-unlock.conf`) with
  `gdm.conf`'s rules for the name, so the service does not depend on `gdm3`.
- `better-os-login-fallback@.service` (`agetty` with `--issue-file
  /etc/issue:/run/better-os/login-fallback.issue` on the instance's VT,
  `Conflicts=getty@%i.service`); `better-os-login-backstop.service` and
  `.timer` (`OnActiveSec=90`; standard programs only; `ExecCondition=` on
  `/run/better-os-greeter/ready-seen` and
  `/run/better-os/login-fallback.active` being absent).
- `/etc/greetd/better-os-greeter.toml`: `vt = 1`, `command =
  "/usr/sbin/better-os-greeter"`, `user = "_better-greeter"`, and
  `initial_session` only when the switch carried GDM's automatic login.
- A sysusers entry for `_better-greeter` (home `/var/lib/better-os-greeter`,
  0700; `video` only if ticket 268 finds `cosmic-comp` needs it); tmpfiles
  entries for `/run/better-os-greeter` and `/run/better-os`.
- The XDG autostart entry for `better-os-remember-login` (ticket 264).
- The debconf template `better-os-greeter/daemon_name` =
  `/usr/sbin/better-os-login-supervisor` and `cosmic-greeter`'s postinst
  code for the shared question; a prerm and postrm that, on removal while
  chosen, choose `gdm3` again and stop nothing.
- Upgrades: `dh_installsystemd --no-stop-on-upgrade
  --no-restart-after-upgrade` for `better-os-greeter.service` (greetd ends
  the running session when stopped); `try-restart` for the supervisor and
  the unlock service.
- `Depends: greetd (>= 0.9.0), cosmic-comp`; built by
  `packaging/build-deb.sh` and checked by `verify-deb.sh`: the alias
  target, no `PartOf=` in the supervisor's unit, no
  `After=plymouth-quit-wait` and no `OnFailure=` but `plymouth-quit.service`
  in the greeter's unit, `Restart=always` and `OnFailure=` the backstop in
  the supervisor's, `Type=dbus` in the unlock service's, the bus policy
  file, the template.

## Done when

- [ ] In the VM: install leaves GDM chosen; `dpkg-reconfigure
      better-os-greeter` makes the supervisor the display manager; a
      reinstall of `gdm3` keeps it; `dpkg-reconfigure gdm3` puts GDM back;
      purge leaves the four places of ADR 0044's step 1 as they were before
      install.
- [ ] Upgrading `better-os-greeter` while a person is logged in through it
      (GNOME and the Better OS session, each locked once during the upgrade)
      leaves the session running and unlockable.
- [ ] Removing `gdm3` on Ubuntu leaves the unlock service owning its name.
- [ ] A greeter that cannot start ends in the text login, never in GDM.
