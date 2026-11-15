# ADR 0029: Hardware writes happen in a helper unit that sees one file

## Status

Accepted on 2026-11-15 by the owner (ticket 86). Narrows ADR 0017's unit
plan; ADR 0007's protocol is unchanged.

## Context

ADR 0017 gave `manager-daemon` two hardware writes, the battery charge limit
and the fan mode, and planned to keep `ProtectKernelTunables=yes` on its unit
with `ReadWritePaths=` for the two attribute files, so the rest of `/sys`
stayed read-only.

Ticket 86 checked that in a disposable Ubuntu 24.04 VM (systemd
255.4-1ubuntu8.18, kernel 6.8.0-147) and in a container of the same systemd.
`ProtectKernelTunables=yes` does not make `/sys` read-only there: a unit with
it wrote `/sys/kernel/mm/transparent_hugepage/enabled`. systemd's debug log
says `/run/systemd/mount-rootfs/sys (read-only) is duplicate.`: the option
implies the API file system mounts (`MountAPIVFS=`), whose `/sys` bind wins
over the read-only entry. `ReadOnlyPaths=/sys` is dropped the same way, and
`ProtectControlGroups=yes` implies the same mounts. So the daemon, which runs
dpkg maintainer scripts as root, could write any sysfs file, and the planned
`ReadWritePaths=` lines narrowed nothing.

A read-only bind of `/sys` (`BindReadOnlyPaths=/sys`) is not dropped: with
it, every sysfs write from the unit failed, `ProtectKernelTunables=` and
`ProtectControlGroups=` included.

## Decision

- `better-manager-daemon.service` and `better-hardware-restore.service` keep
  `ProtectKernelTunables=yes` (for `/proc/sys` and the rest of `/proc`) and add
  `BindReadOnlyPaths=/sys`. Neither has a `ReadWritePaths=` or `BindPaths=`
  under `/sys`. The daemon, the restore, and every process they run, dpkg's
  maintainer scripts included, cannot write sysfs.
- A hardware write is made by a transient oneshot unit started with
  `systemd-run --wait --pipe`. Its `/sys` is `TemporaryFileSystem=/sys:ro`
  with exactly the one attribute file bound in (`BindPaths=`, colons escaped),
  `MountAPIVFS=no`, `ProtectSystem=strict`, `ProtectHome=yes`,
  `PrivateTmp=yes`, `PrivateNetwork=yes`, `PrivateDevices=yes`,
  `NoNewPrivileges=yes`, an empty `CapabilityBoundingSet=`, and
  `RestrictAddressFamilies=AF_UNIX`. It runs
  `/usr/libexec/better-manager-daemon --write-attribute PATH VALUE`, which
  checks again that the path is `charge_control_end_threshold` with 60, 80,
  or 100, or `pwm1_enable` with 0 or 2, under `/sys/devices/`, writes it,
  and prints what it reads back.
- The daemon checks the argument against its closed set and asks polkit
  first (ADR 0017), finds the attribute itself, resolves it to its real
  device path, and only then starts the helper. Nothing the caller sends
  becomes a path.
- The boot-time restore starts the same helper.

## Consequences

- A write costs one transient unit, a few tens of milliseconds; the two
  controls are changed by a person, not in a loop.
- The daemon needs systemd's bus to start the helper (the restore unit is
  ordered after `dbus.service`). A failure to start it is a failed write with
  systemd-run's own message in the details.
- `packaging/hardware-unit/check.sh` is the proof and is run in a VM and in a
  container: the daemon's and the restore's sandboxes cannot write a sysfs
  file, `/proc/sys`, or the attributes themselves; the helper writes its one
  file and cannot write another sysfs file, another attribute, or create
  anything under `/sys`; the helper refuses any file but the two.
- Offering another hardware attribute means a new helper rule and a new
  polkit action, and a change to this record.

## Not chosen

- `ReadWritePaths=` under `ProtectKernelTunables=` (ADR 0017's plan): inert on
  systemd 255, as above.
- Dropping `ProtectControlGroups=` and using `ReadOnlyPaths=/sys`: it works,
  but it trades one protection for another and breaks again the moment any
  option that implies the API mounts is added.
- A permanent helper service: nothing needs to run between two writes.
