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

- [x] Every fact above recorded in Notes for both bases, with the commands and
      the outputs in `/home/tim/Developer/better-os-evidence/201/`.
- [x] The spec's process table updated: each **unverified** mark this ticket
      settles either removed with the evidence or turned into the failure it
      found.
- [x] Each gap the inventory found has its own ticket in this change, and a
      blocking fault (a binary that does not run, GDM refusing the session, a
      Chinese input case failing) is brought to the owner before any other
      session ticket starts, with **Blocked:** lines on the tickets it stops.
- [x] The VM, the images, and the packages deleted after, and the freed space
      recorded.

## Notes

Run on 2026-11-21. No blocking fault: the closure installs on both bases
without touching a base package, every binary runs on the bases' libraries,
GDM lists and starts the session, and none of the four Chinese input faults
reproduced. Nothing here stops another session ticket. Every record is in
`/home/tim/Developer/better-os-evidence/201/` (`ubuntu/`, `zorin/`,
`common/`, `scripts/`); the scripts are committed as
`packaging/tests/session-vm/` (`vm.sh`'s header is the run order for tickets
203 to 205).

**The machines.** QEMU 8.2.2 with KVM, `-m 6G`, 4 vCPUs, virtio-vga at
1920×1080, headless (QMP screendumps and keys, ssh on a loopback port), each
under `systemd-run --user --scope -p MemoryMax=7G -p MemorySwapMax=0`, one at
a time, ended by PID.

- Ubuntu 24.04.5: `noble-server-cloudimg-amd64.img` (SHA256SUMS checked with
  `gpgv` against the host's `ubuntu-cloudimage-keyring.gpg`) in a 24 GiB
  sparse qcow2 overlay with `discard=unmap`, cloud-init, then
  `ubuntu-desktop-minimal` (GNOME 46, GDM 46.2, the Firefox snap) installed
  inside: smaller to fetch than the 6 GB desktop ISO, and the result is the
  desktop installer's "minimal installation". Run twice: the checks, then the
  hour of use below.
- Zorin OS 18.1 Core: the ISO from the mirror `zrn.co/18core64` redirects to
  (`free.nchc.org.tw`, one of the mirrors `zorin.com/os/download/18/core/`
  lists), checked against the SHA256 on help.zorin.com (Zorin publishes no
  signature), booted as the live system with no disk. Two differences from an
  installed Zorin, both handled: casper writes `WaylandEnable=false` and
  autologin into `/etc/gdm3/custom.conf`, which hides every Wayland session
  (the owner's installed Zorin 18 has Wayland on, read from this host's
  `/etc/gdm3/custom.conf`), so both lines were taken out and GDM restarted;
  and the live user got a password so GDM's PAM could unlock the keyring.

**The install as the session will do it** (`guest/install-closure.sh`):
the deb822 source, a keyring holding only `63C4 6DF0 140D 7389 6142 9F4E 204D
D8AE C33A 7AFF` (from keyserver.ubuntu.com, checked against the signed
`InRelease`), the pin with the 14 names at 500 first and the origin at −1
after it, `shared/default-x-display-manager` preseeded to `gdm3`, then the 14
and the six Fcitx 5 packages. Ubuntu: 101 packages added, none removed, none
changed; Zorin: 80 added, none removed, none changed. Exactly the 14 came from
System76 on both. `apt-get -s dist-upgrade` afterwards takes nothing from
System76 (on the Zorin live image it lists 632 Ubuntu and Zorin updates the
ISO is behind on, the same 632 as before the install). `display-manager.service`
stayed `gdm.service`; `greetd` came in as `cosmic-greeter`'s dependency and
stays disabled. System76 publishes all 14 for `arm64` at the amd64 versions
(`common/closure-amd64-arm64-versions.txt`). Removal (`guest/remove-closure.sh`)
purged the added packages, the source, the key, and the pin: the package list
equals the one before on both bases, once the test tools installed by hand
afterwards were taken out too (Ubuntu: my purge of `strace` took
`ubuntu-standard` with it and both were put back; Zorin: `xterm` and its
`libutempter0`). The second Ubuntu run gave the same 101/0/0/14.

**The stand-in session** (`guest/session-files.sh`): the session file, a
launcher that sets the spec's environment and runs `cosmic-comp` with a
"ready" program, which imports the variables, starts a target bound to
`graphical-session.target`, and runs `cosmic-settings-daemon`, `cosmic-osd`,
`cosmic-idle`, `cosmic-bg`, `cosmic-greeter`, Fcitx 5, and a terminal; the
portal through D-Bus activation; the `betteros` dconf profile and
`betteros-portals.conf`.

**The facts** (each in the spec's process table with its evidence):

- GDM lists 「Better OS」 and starts it on both bases, and preselects it at
  the next login. A greeter already running when the session file appeared
  did not list it until GDM restarted (Ubuntu, screenshots 03, 04); the real
  install-then-log-out path is ticket 205's. After removal AccountsService
  still said `Session=better-os` and GDM fell back to the default session.
- Every ELF program of the closure finds all its libraries (`ldd`), and the
  journal has no missing-symbol line, on both bases (`out/inv1/ldd.txt`,
  `journal-missing-symbols.txt`).
- XWayland starts eagerly with the compositor; `DISPLAY=:0` reaches clients
  through the environment `cosmic-comp` gives the ready program, imported into
  the user manager and D-Bus activation.
- Chinese input: Zhuyin typed and committed into GTK (zenity, read back),
  Qt 5 on XWayland and (Ubuntu) on Wayland, xterm over XIM (read back),
  Firefox (Ubuntu's snap; Zorin 18.1 Core ships Brave and no Firefox, so
  Brave on Zorin), and Electron 33 on XWayland and on Wayland with
  `--wayland-text-input-version=3` (read back) — on both bases. Shift alone
  switches Chewing and English and back, as the owner does in IBus. The four
  upstream issues did not reproduce: 2947 and 2952 (letters and Enter reached
  every application with Chewing active), 2778 (GTK and Electron menu bars
  opened while Fcitx 5 ran), 2702 (locked with 你好 in composition; the
  password went to the locker and unlocked). One fault found instead: with
  `GTK_IM_MODULE=fcitx`, as the spec had it, the snap Firefox has no input
  method at all (AppArmor denies its D-Bus call to Fcitx 5); unset, it types
  Zhuyin through text-input-v3. The spec now says not to set it.
- Screen casting through the portal on PipeWire 1.0.5 gave a 1920×1080 frame
  to `pipewiresrc` on both bases (`scripts/screencast-check.py`).
- Locking: with no locker started, `loginctl lock-session` does nothing.
  `cosmic-greeter` is the locker when it runs inside the session's scope;
  started as a user unit it exits (`NoSessionForPID`). It never sets
  `LockedHint`.
- gnome-keyring is unlocked by GDM's PAM at login and by `cosmic-greeter`'s at
  unlock (`gkr-pam: unlocked login keyring`), on both bases.
- `cosmic-config` reads system defaults through `XDG_DATA_DIRS`, key by key
  (`strace` of `cosmic-bg` with a probe directory first).
- `cosmic-comp`'s output configuration is `~/.local/state/cosmic-comp/outputs.ron`
  (format in the spec's display row).
- `tracker-miner-fs-3` does not start in the session: its unit is
  `WantedBy=gnome-session.target` and its autostart entry excludes the
  session's desktop names.
- XDG autostart entries that ran, and `graphical-session.target`'s other
  units, are listed in the spec's process table and `out/inv1/autostart.txt`.
- `system_actions`: COSMIC's defaults name five programs the closure does not
  install (`cosmic-app-library`, `cosmic-launcher`, `cosmic-screenshot`,
  `cosmic-term`, `cosmic-workspaces`); the rest name installed programs.
- Orca, started by hand, read a GTK dialog on both bases.
- What the session wrote that GNOME reads, from home snapshots before the
  first Better OS login, after an hour in the session, and after the GNOME
  login that followed, on each base (a fresh Ubuntu VM and a fresh Zorin live
  boot; `guest/hour-of-use.sh` started and closed editors, terminals, the file
  chooser, COSMIC Settings' pages, the browser, and a Qt window for 60
  minutes, after switching to light mode and another theme in COSMIC
  Settings): the spec's table in "Removing, and returning to the previous
  desktop". GNOME's `~/.config/dconf/user` kept its checksum through each
  hour; no `~/.config/gtk-3.0` or `gtk-4.0` file was written. The scripted
  hour gives no keyboard or mouse input, so it is an hour of the session's
  own background behaviour and of opening its settings pages, not of a person
  typing.
- Idle: 30 minutes after the last input the Ubuntu VM suspended (the run
  was woken through QMP and finished with a sleep inhibitor; its resume left
  a hung kernel worker, `ubuntu/serial-after-vm-suspend-hung-task.log`). On
  Zorin, with the inhibitor held from the start, the session locked itself
  on idle and then a client of the person's asked logind to suspend ignoring
  the inhibitor, so polkit asked for an administrator's password, every 30
  minutes (`zorin/out-hour/polkit.log`). On Ubuntu the logout after the hour
  crashed `systemd-logind` (`Assertion '!q->denied_action' failed at
  src/shared/bus-polkit.c:254`, core in `ubuntu/out-hour/`) and GDM's greeter
  did not come back until a reset; the Zorin logout with the inhibitor held
  did not. Ticket 256.
- Not exercised: Wi-Fi and Bluetooth prompts (wired VM network, no Bluetooth
  adapter), screen blanking short of suspend, real GPUs, arm64, and a
  touchpad.
- Found in passing: `xdg-desktop-portal-cosmic` segfaulted once when the
  compositor ended at logout (Ubuntu, core in `ubuntu/out/`); a logout once
  left `cosmic-settings-daemon` alive for 90 s.

**The duty inventory**: the spec's process table was checked row by row
against what ran (`out/inv*/`). Gaps found, each with its ticket: the
screenshot key (253), the Background, GlobalShortcuts, Wallpaper, and
InputCapture portals (254), autostart entries that collide with the session
(`nm-applet` as a second secret agent, GeoClue's demo agent, Zorin's
`touchegg`) (255), idle suspend against an inhibitor and the logind crash
(256); and for ticket 203 (in its new section): the locker in the
session scope, `GTK_IM_MODULE`, the Secret portal line, Tracker, the
`system_actions` defaults, and ending the launcher's children. Ticket 217 has
a note about `nm-applet`.

**Deleted after**: both Ubuntu images (5.09 GiB and 5.45 GiB on disk; 5213
and 5584 MiB freed), the cloud image both times (593 MiB each), and the Zorin
ISO with its extracted kernel both times (3827 and 3826 MiB freed). No VM
runs; `~/.cache/better-os/orch/vm201/` keeps only the SSH key, the test
password, System76's key and index files, and the Ubuntu checksum files
(740 KiB). Nothing was installed on the host; the host was only read
(`/etc/gdm3/custom.conf`, the keyrings under `/usr/share/keyrings`).
