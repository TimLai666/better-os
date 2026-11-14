# Ticket 97 — Better Monitor reads all the hardware and lives in the taskbar

**Blocked by:** 81 · **Status:** released in v0.2.13

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
- NPU (owner, 2026-11-12): the AMD XDNA NPU through the `amdxdna` driver
  (`/sys/class/accel/accel*/device`: model `vbnv`, firmware `fw_version`,
  power state and runtime status, read on the owner's machine as
  `RyzenAI-npu4`, firmware 1.0.0.63, `D3hot`/`suspended` when idle), and its
  busy time and per-process use from the DRM fdinfo of `/dev/accel*` where the
  driver reports it; whatever the driver does not expose is said plainly
  rather than estimated. Intel NPUs (`intel_vpu`) the same way.
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

## What was built

### Collection (`monitor-collectors-linux`, `monitor-service`)

Five collectors join the six that existed, and the process table gains four
readings. Everything is read unprivileged from `/proc`, `/sys`, and one
netlink dump; nothing runs a command.

- **`linux.gpu`** — one entity per `/sys/class/drm/cardN` (connectors left
  out), keyed by PCI address. amdgpu: `gpu_busy_percent`, dedicated
  (`mem_info_vram_*`) and shared (`mem_info_gtt_*`) memory, the hwmon `edge`
  temperature, the graphics clock (`freq1_input`, else the starred
  `pp_dpm_sclk` line), the memory clock (`pp_dpm_mclk`), and power
  (`power1_input`/`_average`). Intel i915/xe: busy share summed from the DRM
  client engine counters (nanoseconds, or `drm-cycles` over
  `drm-total-cycles`, divided by `drm-engine-capacity`), clock from
  `gt_cur_freq_mhz` or `tile0/gt0/freq0/cur_freq`, power from the hwmon
  `energy1_input` counter over the interval. NVIDIA: `libnvidia-ml.so.1` is
  opened with `dlopen` when it exists (name, utilization, memory,
  temperature, clock, power, matched to the card by PCI address); without it
  the card is named and its busy share says the library is missing. Each
  card records where its busy share came from (`driver`, `clients`, `nvml`,
  `asleep`). NVML is asked only about the NVIDIA cards that are awake, each
  by its PCI address (`nvmlDeviceGetHandleByPciBusId_v2`), never by
  enumerating every card, because opening a card through NVML wakes it. A
  card whose `power/runtime_status` is `suspended` is reported
  asleep and idle and none of its files are read, because reading them wakes
  a powered-down discrete card; NVIDIA's library is only asked when some
  NVIDIA card is awake. "No program has it open" reads as idle only
  for drivers known to publish client engine time (amdgpu, i915, xe,
  amdxdna, intel_vpu); for any other it is "not reported".
- **`linux.npu`** — one entity per `/sys/class/accel/accelN`: driver, vendor,
  model (`vbnv`), firmware (`fw_version`), PCI power state, runtime status,
  and the share of time it was powered up (`power/runtime_active_time`),
  reported as "awake time", not busy. Busy share comes from `intel_vpu`'s
  `npu_busy_time_us`, or from the DRM client keys of open `/dev/accel*`
  files. With no client open it is 0%; with clients whose driver reports no
  engine time (amdxdna today) it says the driver does not report it.
- **`linux.battery`** — every `power_supply` battery (charge, status, energy
  now/full/design in Wh — a charge-reporting battery converted through its
  design voltage, as UPower does — health, draw, time to empty or to full
  honouring `charge_control_end_threshold`, voltage, charge limit,
  technology, model, manufacturer, whether it is a peripheral's) and every
  power source (mains or USB-C: online, negotiated USB type, maximum power).
  A firmware `cycle_count` of 0 is reported as not counted.
  `power.on_battery` and `power.battery.draw` are system readings.
- **`linux.sensors`** — every hwmon input (temperatures with their `_max` and
  `_crit` thresholds, fans, voltages, currents, power, clocks) in plain SI
  units with the driver's label, keyed `chip/input`; two chips with one name
  are told apart by their device. A chip whose read takes over a millisecond
  (ACPI and embedded-controller chips: `acpitz`, `acpi_fan`, `asus`, a
  battery's hwmon) is read at most every five seconds; on the owner's
  machine that took the collector from about 25 ms a round to under 1 ms.
- **`linux.process.usage`** — what each process does with the network, the
  graphics card, and the NPU, written into the process table's own
  entities: `process.network.receive.rate`/`send.rate` (TCP only, from a
  `NETLINK_SOCK_DIAG` dump's `tcp_info` byte counters matched to socket
  inodes under `/proc/<pid>/fd`), `process.gpu.utilization` (busiest engine)
  and `process.gpu.memory.dedicated`/`shared`, `process.npu.utilization` and
  `process.npu.memory`. Descriptor links are re-read every five seconds (a
  new process is read the round it appears); the socket table is dumped every
  three seconds because the kernel walks its whole connection hash table for
  a dump, about 9 ms here however few sockets are open. A DRM client shared
  by two descriptors or processes is counted once. Other users' processes
  read as permission denied, never as zero.
- **`process.energy.power`** — on battery, the battery's draw shared out by
  each process's processor plus graphics time; on mains, or without a
  battery, it says why there is none. The service adds it up by process name
  since the machine was last unplugged (`StatusDocument.battery_use`, kept in
  memory).
- `Roots` gained a socket source (the kernel, or a recorded dump file) and an
  NVML switch that is off for every snapshot, so no test can open a netlink
  socket or load a vendor library. `monitor-core` gained the `Gpu`, `Npu`,
  `Battery`, `PowerSource`, and `Sensor` entity kinds and the volt, ampere,
  RPM, and watt-hour units.
- `QueryStatus` takes an optional `top_processes`: the round comes back with
  only the busiest few processes per resource (`monitor_ipc::trim_round`),
  which is what the taskbar item and the popup ask for.
- `better-monitor.service` now allows `AF_NETLINK` beside `AF_UNIX`, for the
  socket dump.
- **History is schema 2, in its own directory.** The new entity kinds cannot
  be decoded by v0.2.11, so the store moved from
  `$XDG_STATE_HOME/better-monitor/` to `$XDG_STATE_HOME/better-monitor/schema-2/`.
  On first start the new service reads the old history, incidents, and
  inventory read-only (no torn tail truncated, nothing written) and copies
  them in; a rollback to v0.2.11 opens its own files exactly as it left
  them. The import is finished only when the marker file
  `imported-from-schema-1` is written, after all three logs are rewritten
  and synced; a crash before that runs the import again at the next start,
  and records already copied are recognised (history by time, incidents by
  the moment they were marked, inventory by capture time) so nothing is
  doubled. Imported incidents are renumbered after the highest number the
  new store already has, so an incident marked before a deferred import
  keeps its number and both are kept. An old log that exists but cannot be
  read is logged and the marker is not written, so the next start retries;
  only a missing file means "nothing to import". Version 1 records read as
  version 2 unchanged.
- **A record the store cannot decode is kept, not dropped.** The reader
  leaves it out of what it returns instead of failing the whole open, but
  its original bytes go into `<log>.undecodable` beside the log (appended,
  deduplicated, synced, and cut back to its last whole frame first if a
  crash tore one) before any compaction rewrites the log, so a later
  version can still read it. The count is `StoreStats.set_aside_records`
  in the status reply; the service writes one journal line when it is not
  zero, and the History page shows a warning bar with the count.
- **One writer per store.** Opening a store takes an exclusive `flock` on
  `<store>/lock`; a second open gets `StoreError::Locked`. A service that
  finds the store locked takes its bus name at once (normally it serves
  its interface first and takes the name last), then asks for the store
  again every 2 s for a minute and serves the interface once it has it.
  The window's own engine uses the shared store only while Better Monitor
  is switched off and the lock is free; otherwise it records into a private
  directory of its own, `$XDG_RUNTIME_DIR/better-monitor/better-monitor-window-<pid>-<random>`
  (0700; in the temporary directory with a random name when there is no
  runtime directory), removed when it stops; directories of this user's
  windows that crashed are removed when the next one starts. There is no
  legacy import into it. The window watches `NameOwnerChanged` for the
  service's name and also checks every five rounds whether the service is
  on the bus, or whether the component has been switched on since it
  started; if so it stops its
  engine, connects to the service (ten tries a second apart), and the
  window says it is reading from the service. A component that was already
  on with a service that keeps failing is not retried every few seconds.
  `better-monitor-cli --store` elsewhere starts empty, as before.

### The window (`monitor-gui`)

- Overview: the Graphics card card shows its busy share, a live chart, memory,
  and temperature; the Battery card shows the charge, its state, and the time
  left or to full.
- Performance: a tile and a page per graphics card (busy and dedicated-memory
  charts; utilization, dedicated and shared memory, temperature, power, GPU
  and memory clocks, driver, PCI location, programs using it; the apps using
  it most), per NPU (busy chart, power state, awake time, model, firmware,
  driver; a plain note when no program has it open or the driver reports no
  busy time), "Temperatures and fans" (every temperature on one chart with a
  legend, hottest and fastest-fan figures, then temperatures, fans, power,
  voltages, currents, and clocks, each named by what the chip is — Processor,
  Graphics card, SSD, Wi-Fi, ASUS embedded controller — and its label), and
  Battery (charge chart; state, time, power, health; full and design
  capacity, cycles, voltage, charge limit, model; power adapters and USB-C
  ports; and **What is using the battery**: right now, from this round's
  estimate, and since unplugged, from the service, both with the estimate's
  method stated).
- Processes gained Network and GPU columns; Details gained Disk (reads and
  writes together, so the table still fits at 960 pixels), Network, and GPU.
- Integrated graphics are named from the processor's model when the driver
  gives no name (`AMD Ryzen AI 9 HX 370 w/ Radeon 890M` → `AMD Radeon 890M`).
- **`better-monitor --popup [--at X,Y]`** is the taskbar popup: Processor,
  Memory, Graphics card, Disk, Network, Battery, Temperatures, and Fans —
  whichever the machine has — each with its figure, a five-minute sparkline
  from the service's stored history (full the moment it opens), and its three
  busiest programs (processes of one name added together), or for
  temperatures and fans the readings themselves, then Open Better Monitor.
  It asks for the top-right corner of the display the click was on and closes
  when it loses focus or on Escape. **GNOME does not let an application place
  a window next to the tray, and Wayland lets no application place a window
  at all**, so on GNOME the popup goes where Mutter puts new windows; the
  requested position applies under X11 and to compositors that honour it.
- Opening the window starts the taskbar item when no item owns
  `org.betteros.MonitorTray1`: it asks the bus for that name, and the
  activation file starts it through its user unit. Only a development build
  with no activation file installed runs the program directly. When Better
  Monitor is switched off in Better Manager (its unit installed and not
  enabled), the window starts neither the item nor the service: it uses a
  service that is running anyway, and otherwise collects for itself and says
  why.
- The GPU column (Processes and Details) appears only when some program has
  a GPU reading, so a card whose driver reports nothing per program (or a
  card that is asleep) does not leave a column of dashes. The Graphics card
  and Battery cards on the Overview only appear on a machine that has a
  card or a battery, as on the Performance page and in the popup. A process reads "not
  reported", never 0%, unless some card (or NPU) has a driver that publishes
  per-program engine time; a card that only reports its own busy share
  (`gpu_busy_percent`, NVML) says how busy the card is, not which program
  made it so.
- A service of another protocol version (an update waiting for the service
  to restart) makes the window collect for itself with a sentence saying so,
  instead of failing to read a reply.
- Every new string is in English and Traditional Chinese, and every collector
  has a name on the "Where the numbers come from" list.
- `better_ui::fluent::chart::ChartHue` gained Red (temperatures) and Magenta
  (NPU); the kit's colour test covers them.

### The taskbar item (`crates/monitor-tray`, `better-monitor-tray`)

- A StatusNotifierItem and a `com.canonical.dbusmenu` menu written on zbus in
  the same way as `awake-tray`, registration verified against the watcher's
  own list. It never exits for want of a watcher: it subscribes to
  `NameOwnerChanged` for `org.kde.StatusNotifierWatcher` before its first
  attempt and registers each time the name gets an owner, so an item that
  started before the panel at login appears when the panel does, and a panel
  restart (seen with swaybar) gets it again (`monitor_tray::keep_registered`,
  the same shape as Better Awake's in ticket 102). The tooltip
  is announced on its own when its text changes.
- The icon is drawn as `IconPixmap`, 22 and 44 pixels, **square**: swaybar
  ignores a pixmap that is not square and GNOME's AppIndicator extension fits
  icons into a square slot, so a Stats-style wide icon is not possible. Bars
  style: one bar per chosen reading filled to its share (rates on a log
  scale to 100 MB/s, temperature out of 100 °C) in that reading's chart
  colour. Numbers style: the first two chosen readings as two rows of
  double-size digits in their colours. The default is processor and memory
  as bars. Everything drawn has a one-pixel dark edge, so the light ink reads
  on a light panel as well as a dark one. It is redrawn at most once a
  second (one summary request a second, no processes) and announced only
  when the pixels changed.
- Left click opens or closes the popup at the click point; a click within
  half a second of the popup closing itself is the click that took its focus,
  and opens nothing, so clicking the icon to close the popup does not reopen
  it. Middle click opens the window. The right-click menu lists every reading as text (processor,
  memory, graphics, disk, network, battery with time left, temperature,
  fan), then Show all readings, Open Better Monitor, "Show in the taskbar"
  (a checkmark per reading, the last one cannot be hidden, readings the
  machine lacks are disabled; bars or numbers as radio items), and Quit. The
  choice is kept in `$XDG_CONFIG_HOME/better-os/monitor/tray`. The language
  follows the window's own setting, then the environment.
- The popup and the window it opens run in transient scopes of their own
  (`systemd-run --user --scope`), not in the item's unit, so stopping or
  restarting the item never touches them; the unit carries no sandboxing,
  since that would be inherited by the person's window. Without a user
  manager they are started directly, and so they are when `systemd-run` is
  not installed or exits unsuccessfully within 3 s (one fallback, logged to
  the journal). Nothing on the item's D-Bus thread waits for this: the
  click returns once the process runs, and the thread that watches the
  popup starts the fallback.
- It owns `org.betteros.MonitorTray1` (one item per session; a copy still
  exiting is waited for up to five seconds, and a name that stays taken
  makes it exit 1) and runs as the
  user unit `better-monitor-tray.service` (`PartOf` and `WantedBy`
  `graphical-session.target`, `Restart=on-failure`, `KillMode=process` so
  stopping it never closes a window it opened). Better Manager enables and
  disables it with the component, so an update restarts it and disabling
  Better Monitor stops it. `ExecStartPre` ends a copy of this user's that was
  started some other way, matched on the full command line because the
  kernel's 15-character name for it, `better-monitor-`, is also the
  service's. Its requests start the service through the D-Bus activation
  file `org.betteros.Monitor1.service` (`SystemdService=better-monitor.service`).
  When the service does not answer, or speaks another protocol version
  (Better Monitor was updated and the service has not restarted), the menu
  and the tooltip say which, the log says it once, and it asks again every
  five seconds.

### Packaging

`build-deb.sh` builds `monitor-tray` and ships `/usr/bin/better-monitor-tray`,
`better-monitor-tray.service`, and the two activation files
(`org.betteros.Monitor1.service`, `org.betteros.MonitorTray1.service`) in
`better-monitor`; `verify-deb.sh` requires them, checks what the unit runs,
its `ExecStartPre`, `PartOf`, and `WantedBy`, that no autostart entry ships,
and that each activation file names its bus name and unit; the manifest's
`paths` list them. Better Manager now runs `systemctl --user daemon-reload`
before it starts or restarts a component's units
(`manager-platform::user_units`), so an update's changed unit files (this
one's `AF_NETLINK`) and new units (the taskbar item's) take effect.
`monitor_ipc::PROTOCOL_VERSION` is now 2. Cargo.lock gained only edges between
existing packages (libc and tempfile for the collectors, zbus for the window,
the new workspace crate); `docs/third-party-licenses.md` was regenerated.

### Tests

The collectors are tested against trees recorded on the owner's ASUS
Vivobook S 14 and written into temporary directories by the tests
(`tests/hwmon.rs`, `battery.rs`, `gpu.rs`, `npu.rs`, `netproc.rs`,
`energy.rs`), plus synthetic Intel, NVIDIA, Intel-NPU, and missing-interface
cases; the sock_diag reply is built byte for byte. The service tests cover
the trimmed summary; the taskbar item is driven on a private session bus
against a fake watcher (`crates/monitor-tray/tests/tray.rs`); the window's
new decisions (titles, units, sections, history lines, placement, battery
lines) are tested without a window. The collector-set and overhead tests that
used to read the machine running them now use the recorded `snapshot-a` and
`snapshot-b` trees; the two real-machine overhead measurements remain as
`#[ignore]` tests to run on purpose.

### Cost while the window is closed

Measured on the owner's machine (Ryzen AI 9 HX 370, 24 logical processors)
on 2026-11-13: the release `better-monitor-service` from this branch ran on a
private session bus with no client at all (no window, no taskbar item),
scratch `HOME`/XDG, sampling once a second as it does in production, and its
own `/proc/<pid>/stat` user plus system time was read over 120 seconds after
a 20-second warm-up. Over the same 120 seconds the installed v0.2.11 service
(the one the owner runs) was read the same way, so both saw the same load
(load average 5–10 from other work on the machine).

| | CPU time in 120 s | Share of one core | Share of the machine | Resident memory |
|---|---|---|---|---|
| This branch | 6.68 s | 5.6% | 0.23% | 29 MB |
| Installed v0.2.11 | 4.85 s | 4.0% | 0.17% | 177 MB* |

So the new collection costs about 1.5% of one core (15 ms of CPU a second).
Per round, from `cargo run --release --example overhead` and a timed probe:
the process table (unchanged) about 20 ms; `linux.process.usage` about 1 ms
in most rounds, about 10 ms in the rounds that dump the socket table (every
3 s) and about 14 ms in the rounds that re-read every descriptor link
(every 5 s); `linux.sensors` under 1 ms, 8–12 ms in the rounds that re-read
the slow ACPI chips (every 5 s); `linux.gpu` under 1 ms; battery 0.3 ms.
\*The installed service's memory includes hours of history it has loaded;
this branch's started empty. The taskbar item's own cost was not measured
in release form.

### Not done, and why

- **UDP traffic is not attributed to programs.** The kernel keeps no per-socket
  byte counter for UDP, so a browser's QUIC traffic is missing from the
  Network column; the column's tooltip says TCP only. Attributing it needs
  eBPF or cgroup accounting with privilege.
- **NPU busy time on this machine is "not reported" whenever a program uses
  it**: amdxdna publishes no busy counter in sysfs and no program here had
  the NPU open to show what its fdinfo carries. Awake time and power state
  are real.
- **The energy estimate is an estimate**: the screen and radios are not
  charged to any program, and only processes this user can see share the
  draw. "Since unplugged" is kept in memory and restarts with the service.
- **The tray icon cannot be as wide as Stats'** (see above), and Numbers
  style shows two readings.
- `delivery-status.md` was not updated (outside this ticket's files).

### Not verified

- **A GNOME desktop.** Everything was seen in a private headless sway with
  swaybar as the tray host. swaybar draws no dbusmenu menus, so the
  right-click menu was read over D-Bus (`GetLayout`) and driven with `Event`
  calls rather than seen; how GNOME's AppIndicator extension routes a left
  click (Activate versus the menu) was not observed.
- **Closing on focus loss and on Escape**, and the click debounce in a real
  panel. GPUI reports a Wayland window active once it has keyboard focus,
  and the headless session has no keyboard, so the popup never became active
  there and these paths were not exercised; the debounce is tested against a
  stand-in process.
- **NVIDIA (NVML) and Intel GPUs and Intel NPUs on real hardware**: only the
  owner's AMD machine; those paths are tested against synthetic trees, and
  NVML not at all beyond "library absent" and the choice of which cards
  to name to it.
- **The window handing over to the service** and the service waiting for
  the store lock: the decisions (`embedded_store`, `handover`) and the lock
  itself are tested; the handover was not watched end to end with a real
  service starting while a window collects.
- **The `systemd-run` fallback** under a real user manager: tested with
  stand-in programs that are missing or exit 1 after 300 ms.
- **The window reacting to `NameOwnerChanged`**: the service taking its
  name while it waits for the store is tested on a private bus; the
  window's side of it was not run against a real session bus.
- **The installed package**: D-Bus activation of the service was proven on
  the private bus with a copy of the activation file whose `Exec` points at
  the development build; the taskbar unit, its `ExecStartPre`, and the tray's
  own activation file were not run under a real user manager. The packages
  were built with `build-deb.sh --target ubuntu-24.04` and all nine pass
  `verify-deb.sh dist ubuntu-24.04`, but were not installed.
- **A runtime-suspended discrete card**: this machine's only card is
  integrated and always active; the "asleep, not read" path is tested
  against a tree.
- The click debounce in a real panel (see focus loss above).
- The popup's requested position: sway ignores it; the screenshots place it
  with a sway rule.

### Screenshots

A private headless sway (1600×1000, swaybar as the tray host) on its own
session bus, with `HOME` and the XDG directories in a scratch directory, the
real service and taskbar item running inside it, reading the owner's machine
(on mains for the first shots, then on battery):
`/tmp/claude-1000/-home-tim-Developer-better-os/3b559518-c3b2-4104-bc77-d1c2e845c29a/scratchpad/shots-97/`.

- English, dark, 1600: `en-dark-1600-01-overview` (Graphics card and Battery
  cards with live charts, on battery), `-02-processes` (Network and GPU
  columns), `-03-details` (Disk, Network, GPU), `-04-perf-gpu`, `-04-perf-npu`
  (`RyzenAI-npu4`, firmware 1.0.0.63, D3hot/suspended), `-04-perf-temps` and
  `-04-perf-temps-fans` (every hwmon sensor, two fans), `-04-perf-battery` and
  `-04-perf-battery-use` (on battery: power adapters, "Right now", "Since
  unplugged").
- English, dark, 960: `en-dark-960-02-processes`, `en-dark-960-03-details`
  (every column fits), `en-dark-960-04-perf-gpu`.
- Traditional Chinese, light, 1600: `zh-light-1600-01-overview`, `-02-processes`,
  `-03-details`, `-04-perf-gpu`, `-04-perf-npu`, `-04-perf-temps`,
  `-04-perf-battery`, `-04-perf-battery-use`.
- The popup opened by clicking the taskbar icon in swaybar:
  `en-dark-popup-from-tray`, `zh-light-popup-from-tray`.
- After the review fixes: `tray-outline-dark-zoom`, `tray-outline-light-zoom`,
  and `tray-outline-light-numbers-zoom` (the dark edge on a dark and on a
  light swaybar; the light one is after a bar reload, so the item had
  registered with the new watcher), and `en-dark-1600-01-overview-no-gpu-no-battery`
  and `en-dark-1600-02-processes-no-gpu`: the service run under `bwrap` with
  `/sys/class/drm`, `/sys/class/accel`, and `/sys/class/power_supply` hidden
  behind empty mounts, so the machine has no graphics card, NPU, or battery.
  The Disk column reads "—" there for most programs because the sandbox's own
  `/proc` mount cannot read other processes' I/O counters; that is the
  sandbox, not the window.
- Earlier, the icon, zoomed from the bar: `tray-default-zoom` (processor and memory as
  bars, the default), `tray-bars3-zoom` (processor, memory, battery),
  `tray-numbers-zoom` (Numbers style, `17` and `30`); full-size
  `tray-default`, `tray-bars3`, `tray-numbers`.
