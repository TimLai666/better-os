# Ticket 273 — Better Monitor's memory page shows the modules: slots, speed, type

**Blocked by:** none

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

The old branch read SMBIOS through a new root interface on `manager-daemon`.
That is not needed: on systemd 255 the udev database already holds the
memory devices for an ordinary user (`udevadm info
/sys/devices/virtual/dmi/id` shows `MEMORY_DEVICE_0_SIZE`, `_TYPE`,
`_SPEED_MTS`, `_FORM_FACTOR`, `_LOCATOR`; read on the owner's machine on
2026-11-21). Windows Task Manager's memory page shows the same facts.

## What to build

- A collector in `monitor-collectors-linux` that reads those properties from
  the udev database without privilege and without running a command; what a
  system does not report is shown as not reported, never guessed (Ubuntu
  22.04's systemd 249: establish whether it has them).
- The memory page shows slots used of total, each module's size, type, speed
  and form factor; both languages.

## Done when

- Tests with fixture udev data: soldered memory, two modules, an empty slot,
  no data at all. Seen on the owner's machine after the next release.
