# Ticket 97 — Better Monitor reads all the hardware and lives in the taskbar

**Blocked by:** 81 · **Status:** ready

## Why

The owner wants Better Monitor at least as good as Windows 11 Task Manager and
then better, collecting all the time in the background, with a taskbar icon
whose popup shows the hardware at a glance the way `exelban/stats` (MIT,
macOS) does in the menu bar. Ticket 81 rebuilt the window, but the GPU,
battery, fans, and every temperature except the processor's still read "not
measured", because `monitor-collectors-linux` does not collect them.

`better-monitor.service` already runs in the user session (ticket 79 made
Better Manager start it) and keeps history, so the window shows hours, not
only the last minute. This ticket keeps that service as the one collector.

## What to build

**Collection, in `monitor-collectors-linux` and `monitor-service`:**

- GPU: utilisation, memory, temperature, clock, and power for amdgpu
  (`gpu_busy_percent`, `mem_info_vram_*`, hwmon), Intel (i915/xe through
  hwmon and the fdinfo engine counters), and NVIDIA through NVML when the
  library is present; per-process GPU use from `/proc/<pid>/fdinfo` DRM keys.
- Battery: charge, rate, time left, cycle count, design and full capacity,
  health, charger state, from `power_supply`.
- Every hwmon sensor: temperatures with their labels, fan speeds, voltages
  and power where exposed.
- Per-process network traffic (from the netlink sock_diag counters or
  cgroup accounting, whichever needs no root), and per-process energy
  estimate on battery.
- Everything optional: a machine without a sensor shows nothing for it, never
  a broken control.

**The window (ticket 81's pages):** fill the GPU, battery, sensors pages, add
the network column to processes, and add "What is using the battery".

**The taskbar item, `better-monitor-tray`:**

- A StatusNotifierItem whose icon is drawn live (`IconPixmap`): small bars or
  numbers for what the person picks, CPU and memory by default, like Stats in
  the macOS menu bar; redrawn at most once a second and never when nothing
  changed.
- Clicking it opens a compact popup window in Fluent: CPU, memory, GPU,
  disk, network, battery, temperatures, and fans, each with a sparkline and
  the top processes, and a button that opens the full window. GNOME does not
  let an application place a window next to the tray, so the popup opens at
  the pointer's monitor's top right and closes when it loses focus; that
  limit is stated in the ticket's What was built.
- The right-click menu lists the same readings as text and lets the person
  pick which ones the icon shows.
- It autostarts with the session, starts the service through D-Bus
  activation if needed, and the window starts it when it is not running, as
  Better Awake's tray does.

## Acceptance

- [ ] On the owner's ASUS Vivobook S 14 (amdgpu, k10temp, asus hwmon,
      BAT1), every page shows real values; screenshots.
- [ ] The tray icon shows live CPU and memory in the sandbox and its popup
      opens; screenshots.
- [ ] Collector tests use recorded sysfs and proc trees; nothing reads the
      real machine in tests.
- [ ] The service's CPU cost while the window is closed is measured and
      stated.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
