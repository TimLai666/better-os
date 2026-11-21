# Ticket 265 — The `better-os-greeter` package: unit, configuration, account, and display-manager registration

**Spec:** `design/login-screen-spec.md` section 5.5; ADR 0044 ("What the greeter is")
**Blocked by:** 264, 267, 280, 203

## Why

The greeter has to be a display manager Debian's own switch knows, so that
`gdm3` upgrades keep the choice and `sudo dpkg-reconfigure gdm3` takes it
back.

## What to build

- `better-os-greeter.service`: `ExecStart=greetd --config
  /etc/greetd/better-os-greeter.toml`, Debian's `greetd.service` settings
  (`Conflicts=getty@tty1.service`, `KeyringMode=shared`) except
  `Restart=no` (the supervisor owns every restart), `Wants=` and `After=`
  the supervisor and the GNOME unlock service, `OnFailure=` the supervisor's
  fall back and `plymouth-quit.service`; no `[Install]` section. Never
  `OnFailure=gdm.service`: GDM is not on this path (L8).
- `better-os-login-supervisor.service` (ticket 267),
  `better-os-gnome-unlock.service` (ticket 280), and
  `better-os-login-fallback@.service`, each bound to the greeter's unit so
  they run only while it is the display manager.
- `/etc/greetd/better-os-greeter.toml`: `vt = 1`, `command =
  "/usr/sbin/better-os-greeter"`, `user = "_better-greeter"`.
- A sysusers entry for `_better-greeter` (home `/var/lib/better-os-greeter`,
  0700; `video` only if ticket 268 finds `cosmic-comp` needs it) and a
  tmpfiles entry for `/run/better-os-greeter`.
- The debconf template `better-os-greeter/daemon_name` =
  `/usr/sbin/better-os-greeter` and `cosmic-greeter`'s postinst code for the
  shared question; a prerm and postrm that, on removal while chosen, choose
  `gdm3` again before the files go.
- `Depends: greetd (>= 0.9.0), cosmic-comp`; built by
  `packaging/build-deb.sh` and checked by `verify-deb.sh` (the unit, the
  TOML, the template, the `OnFailure` line).

## Done when

- [ ] In the VM: install leaves GDM chosen; `dpkg-reconfigure
      better-os-greeter` makes it the display manager; reinstalling `gdm3`
      keeps it; `dpkg-reconfigure gdm3` puts GDM back; purge leaves the
      four places of ADR 0044's step 1 as they were before install.
- [ ] A greeter that cannot start ends in the text login, never in GDM.
