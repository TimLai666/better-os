# Ticket 201 — `cosmic-comp` from the pinned closure runs on Zorin OS 18 and Ubuntu 24.04, started by GDM

**Spec:** better-os-session.md S1, S4 (every **unverified** row of the process table)
**Blocked by:** none

## Why

ADR 0038 resolved the 14-package closure against the host's metadata only:
the binaries were built against Pop's newer Wayland, Mesa, and PipeWire, and
whether they run on Zorin's was not run; GDM starting a non-GNOME session was
not run; four Chinese input faults were open upstream. A failure here stops
the whole line, so it is checked before any session code is written.

## What to build

A disposable VM per base (Zorin OS 18 and Ubuntu 24.04 desktop images, QEMU
with KVM under `systemd-run --user --scope -p MemoryMax=7G -p
MemorySwapMax=0`, as ticket 143 ran it, ended by PID, images deleted after),
and inside it, by hand as root in the VM:

- **The install as the session will do it**: System76's source as deb822
  with `Signed-By` a keyring holding only `63C4 6DF0 140D 7389 6142 9F4E 204D
  D8AE C33A 7AFF`, the pin (origin at −1, the 14 names at 500), the
  display-manager question preseeded to `gdm3`, the 14 packages and Fcitx 5
  installed. Record `apt-get -s dist-upgrade` taking nothing from System76,
  `display-manager.service` still GDM, and whether System76 publishes `arm64`
  for the 14 in `noble`.
- **A hand-made session file** in `/usr/share/wayland-sessions/` that runs
  `cosmic-comp` with a terminal, `cosmic-settings-daemon`, `cosmic-osd`,
  `cosmic-idle`, `cosmic-bg`, the portal, and Fcitx 5 (no Better OS code yet),
  `XDG_CURRENT_DESKTOP=BetterOS:COSMIC`, `DCONF_PROFILE=betteros`.
- **The facts** the spec leaves to this ticket, each recorded in Notes:
  GDM lists and starts it; every binary runs on the base's libraries
  (`ldd`, the journal, a missing-symbol search); XWayland starts (eagerly or
  on demand) and how `DISPLAY` reaches clients; the four Chinese input cases
  (Zhuyin through Fcitx 5 into a GTK window, Firefox, and an Electron
  application; a menu opened while Fcitx 5 runs; lock and unlock with a
  composition open — upstream issues 2947, 2952, 2702, 2778); whether
  fcitx5-chewing toggles Chinese and English with Shift as the owner does in
  IBus; screen casting through the portal on the base's PipeWire; which
  process answers logind's `Lock` and draws the lock screen; gnome-keyring
  unlocked by GDM's PAM; whether `cosmic-config` reads system defaults through
  `XDG_DATA_DIRS`; the path and format of `cosmic-comp`'s output (scale)
  configuration; how `tracker-miner-fs-3` starts outside GNOME; which of the
  base's XDG autostart entries and which `graphical-session.target` units of
  other packages start; every `system_actions` entry and the program it names
  (and which exist: the window switcher, screenshot, app library, terminal);
  which files under `$HOME` the session writes that GNOME reads (snapshot
  before and after an hour of use, diffed); Orca reading a GTK window.
- **The duty inventory**: the spec's process table checked row by row against
  what ran, and every duty found that the table misses, each with who could
  provide it.

## Done when

- [ ] Every fact above recorded in Notes for both bases, with the commands and
      the outputs in `/home/tim/Developer/better-os-evidence/201/`.
- [ ] The spec's process table updated: each **unverified** mark this ticket
      settles either removed with the evidence or turned into the failure it
      found.
- [ ] Each gap the inventory found has its own ticket in this change, and a
      blocking fault (a binary that does not run, GDM refusing the session, a
      Chinese input case failing) is brought to the owner before any other
      session ticket starts, with **Blocked:** lines on the tickets it stops.
- [ ] The VM, the images, and the packages deleted after, and the freed space
      recorded.
