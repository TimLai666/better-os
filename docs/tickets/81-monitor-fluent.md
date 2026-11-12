# Ticket 81 — Better Monitor, rebuilt in Fluent and complete

**Blocked by:** 80 · **Status:** merged, not released

## Why

The owner finds Better Monitor unusable. The 2026-11-12 audit found internal
keys such as `cpu.utilization.busy` and raw ratios on the processor page,
mislabelled rows, text that cannot be read on the applications table, tables
that fill half the window, English process states in the Chinese UI, jargon
on the overview, no charts anywhere, navigation that turns into unlabeled
icons below 800px, and no "follow the system" appearance. The owner's rule:
an app is complete before it is called done.

## What to build

Every page of `crates/monitor-gui` rebuilt on `better_ui::fluent` (ticket 80):
NavigationView with the 48px compact mode, settings cards, details tables,
info bars, the shared `words`, `theme::Appearance` (dark, light, system).

The app is complete when it does what Windows 11 Task Manager does, on Linux:

- Overview: one plain sentence on how the machine is doing, then CPU, memory,
  disk, network, GPU, and battery cards with a live chart each.
- Processes and applications: a sortable, filterable table that fills the
  window, with end task, end process tree, open file location, and details;
  states translated and explained.
- Performance: per-resource pages with history charts (CPU overall and per
  core, memory composition, each disk's activity and throughput, each network
  interface, GPU, temperatures and fan speed from hwmon).
- Startup apps and background services the user session runs, with enable
  and disable where it needs no root.
- History and events as they exist today, in plain words.
- Every number has a unit and a human label; no internal key is ever shown.
- zh-TW and en complete; nothing truncated in either at 960px.

## Acceptance

- [ ] Every finding N1–N11 of the audit is fixed, each shown by a screenshot.
- [ ] Every page in light and dark, English and zh-TW, at 1600 and 960 wide,
      screenshotted and looked at.
- [ ] Charts update live without blocking the UI thread.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## What was built

- **The window is rebuilt on `better_ui::fluent`.** Title bar, NavigationView
  (320px with labels from 1008px wide, 48px icons with ☰ and tooltips below
  that, as WinUI's Auto mode does; the person's own choice wins), a Title and
  a command bar on every page with commands that do not fit moved to the
  overflow menu, info bars, settings cards and expanders, combo boxes, a
  toggle switch, a selector bar, content dialogs, and context menus. No
  gpui-component widget is drawn directly any more.
- **Pages.** Overview, Processes (apps and background processes grouped,
  expandable), Details (every process, flat or as a tree), Performance,
  Startup apps, Services, History, Slow moments (the old Incidents), System
  information (the old Inventory), and Settings. The old Diagnostics page is
  now the "Where the numbers come from" expander in Settings, so the pane
  fits a 600px-tall window.
- **Overview.** One plain sentence on how the machine is doing, chosen by
  `i18n::health_sentence` from the resource verdicts so "busy" and
  "programs are waiting" stay apart, with the busiest app named under it.
  Then six cards (processor, memory, disk, network, graphics card, battery)
  with a status word, a figure, a live sparkline, and one line of detail;
  each card opens its Performance page. Then the five apps using the most.
- **Processes and Details** fill the window and scroll inside a fixed
  header. Both sort by any column, filter by name, PID, or user, and offer
  End task (an app's processes, or one process), End process tree (children
  before parents), Suspend/Resume, Lower priority, Open file location, and
  Properties, from the command bar and from a right-click menu. Ending always
  asks first, with Cancel as the default button; anything the controller
  refuses (another user's process, Better Monitor itself) is disabled or
  answered with the reason in an info bar. Process states are translated
  and explained on hover. A process's processor share is divided by the
  logical processor count, as Task Manager shows it, so the column adds up to
  the Processor page instead of to 2,400%.
- **Performance** lists every resource with a small chart, as Task Manager
  does, and shows: processor utilization over 60 seconds or per logical
  processor, speed, processes, threads, up time, maximum speed, run queue,
  and package temperature; memory in use over time, its composition (in use,
  waiting to be saved, cached, free), committed, cached, swap; each disk's
  active time and read/write transfer rate, response time, capacity, and
  type; each network adapter's throughput, connection state, and link speed;
  the processor temperature chart; the graphics cards the inventory names.
- **Startup apps** reads XDG autostart entries from `$XDG_CONFIG_DIRS` and
  `$XDG_CONFIG_HOME` and switches one by writing the person's own copy with
  `Hidden` and `X-GNOME-Autostart-enabled`; a system file is never written.
  **Services** reads `systemctl --user list-units` and `list-unit-files` and
  enables or disables a unit with `systemctl --user`, off the window thread,
  with a 10-second timeout and the `BETTER_OS_TEST_SYSTEMCTL_GUARD` guard the
  manager uses. Units a Better OS package installs (`better-*`,
  `org.betteros.*`) are shown as managed by Better Manager and are not
  switchable here (AGENTS.md); static, masked, and transient units say why
  they cannot be switched.
- **History** draws each stored metric on the clock (`stored::binned`): the
  span is cut into slots, so time nothing was recorded is a gap where it
  happened. Slow moments and System information speak in labels; metric and
  inventory keys the window has no words for are left out rather than shown.
- **Appearance and language.** Dark, light, or "Use system setting", through
  `theme::Appearance` (stored and followed); language System, English, or
  繁體中文, stored beside it. Both switch at once.
- **Charts are live without blocking the UI.** The collectors run on the
  link thread as before. Each adopted round is turned once into
  `charts::Readings` and appended to `charts::LiveHistory` (60 readings per
  series, gaps kept as gaps, unplugged devices forgotten after a minute);
  drawing only reads them.
- **New in the kit.** `fluent::chart` (LineChart with grid, area, dashed
  second series, and gap handling; sparkline; CompositionBar; categorical
  data hues kept apart from the accent; `runs`, `nice_maximum`, `peak`,
  `composition` tested). `DetailsTable` gained rich cells (`TableCell`: tone,
  bold, indent, expander chevron, tooltip), `presorted`, `fill` (virtualized
  with `uniform_list`, scroll kept), `empty`, `on_invoke`, `on_context_menu`
  (right click, Menu key, Shift+F10), Up/Down/Home/End row selection, and
  column tooltips. Existing callers compile unchanged.
- **Audit findings.** N1: the Processor page shows labelled figures with
  units, no keys or raw ratios. N2: table text uses the kit's primary and
  secondary text colors in both appearances, one line per 36px row. N3: the
  tables fill the window. N4: states are translated. N5: the overview
  sentence and status words. N6: charts on Overview, Performance, History.
  N7: compact pane with ☰ and tooltips at 960 and 800. N8: "Use system
  setting", in plain words. N9–N11: columns share one table definition per
  page; the two "Storage" cards are one Disk card.

### Not done, and why

- **GPU utilization, battery, fan speed, and temperatures other than the
  processor's are not measured.** No collector reads them, collectors belong
  in `monitor-collectors-linux`/`monitor-service` (outside this ticket's
  files), and AGENTS.md forbids collector calls in GUI code. The pages exist
  and say so; the GPU page names the cards from the inventory.
- **Per-process network traffic** is not collected, so the process tables
  have no Network column.
- **The gallery** does not show the new chart or the table additions
  (`crates/fluent-gallery` was outside this ticket's files).
- `delivery-status.md` was not updated (outside this ticket's files).

### Not verified

- A GNOME/Mutter desktop: everything was seen in headless sway.
- Startup and service switching against a real user manager: the services
  shown in screenshots come from a fake `systemctl` on `PATH` in the
  scratch session, and the startup apps from fixture `.desktop` files in a
  scratch `XDG_CONFIG_DIRS`/`HOME`. The rules are unit-tested.
- The Better Monitor service path: screenshots ran under `dbus-run-session`,
  so the window collected in-process (the warning bar says so).
- Open file location with a file manager present; Enter/Menu-key paths of
  the table were not pressed by hand (no keyboard injection in the capture
  session).

### Screenshots

A private headless sway at exactly 1600×1000, 960×1000, and 800×1000,
driven through a virtual pointer and captured with wlr-screencopy, with
`HOME` and the XDG directories in a scratch directory and the window under
`dbus-run-session`:
`/tmp/claude-1000/-home-tim-Developer-better-os/3b559518-c3b2-4104-bc77-d1c2e845c29a/scratchpad/shots-81/`.

- Matrix, ten pages each (`01-overview`, `02-processes`, `03-details`,
  `04-performance`, `05-startup`, `06-services`, `07-history`, `08-events`,
  `09-sysinfo`, `10-settings`): `en-dark-1600-*`, `en-dark-960-*`,
  `en-light-1600-*`, `en-light-960-*`, `zh-dark-1600-*`, `zh-dark-960-*`,
  `zh-light-1600-*`, `zh-light-960-*`. Every one was checked to show the page
  its navigation item selects.
- `extra-en-dark-1600-perf-{memory,disk,network,temperatures,gpu,battery,cpu-logical-processors}`,
  `extra-en-dark-1600-details-context-menu`, `-details-properties`,
  `-processes-end-task-confirm`, `-startup-selected`, `-startup-enabled`,
  `-services-selected`, `-services-enable` (the fake `systemctl` recorded
  `--user enable syncthing.service`; its listing is static, so the row does
  not change), `-overview-paused`, `-settings-appearance-open`, and
  `extra-en-dark-800-{settings,overview,pane-opened}`.
- `action-00..02-zh-light-*`: ending a real process the test started
  (`sleep`) from Details in zh-TW: confirmation, then the success bar and the
  process shown as ended. These three were taken in the shared wayhand
  sandbox at 960 wide and are not cropped.
- `raw/live-a-en-dark-1600-overview-t0.png` and `raw/en-dark-1600-01-overview.png`:
  the same Overview 55 seconds apart, the charts grown and the figures moved.

They live in a session scratch directory; copy them somewhere durable before
it is cleaned.

## Independent review, and what it changed

An independent review of the first implementation found the issues below.
Each was fixed in this ticket, and each fix has a test that fails without it
where the behaviour is testable without a window. The branch was also brought
up to date with `main` (ticket 82); the only conflicts were the `list.rs`
imports and the end of the kit's tests, and both sides were kept.

1. **Ending from a paused list could signal a reused PID.** Ending is no
   longer offered while updates are paused (command bar and menus), and a
   confirmation that is answered after pausing is refused. In
   `monitor-actions-linux`, every signal and priority change first reads the
   process's start time and real owner back from `/proc` and compares them
   with the snapshot; a mismatch is reported as the process having gone.
   Signals go through `pidfd_open` + `pidfd_send_signal`, with the identity
   checked after the descriptor is open, and fall back to the check plus
   `kill(2)` only where the kernel has no pidfds. A target without a start
   time is refused. Tested against a real child and a fake `/proc` that
   claims a different start time or owner: the child survives.
2. **End process tree had no guard.** `monitor_core::tree_refusal` refuses a
   tree containing an ancestor of Better Monitor, the user's `systemd`, or a
   session leader; the actions crate reads parent, session, and name from
   `/proc` to apply it, and the window explains the refusal. The
   confirmation names how many processes it ends (and says so when it is
   only the one).
3. **`systemctl` output over a pipe's size stalled until the timeout.**
   stdout and stderr are drained on their own threads while the child is
   waited on. Tested with a script that writes 200 KiB.
4. **CPU share.** Task Manager's rule (100% is every logical processor) is
   applied everywhere a process or app share is shown, including Slow
   moments (using the processor count recorded with the moment). Until the
   count is known the cell is a dash, never a share of one processor.
5. **Work on the window's thread.** History charts are binned once, on the
   link thread, when the reply arrives, against the time it was fetched
   (`stored::HistoryView`). Each round's overview, readings, and both tables
   are built on a background thread (`charts::BuiltRound`) with the tables'
   current sort, filter, tree mode, and open groups, and adopted in one step;
   a round that finishes after a newer one is dropped, and settings changed
   meanwhile are applied on top.
6. **Open file location** now asks `monitor-actions-linux` for the folder,
   so the window never reads `/proc`. The path stays an `OsString` end to
   end, ` (deleted)` is stripped from the raw bytes, and the `xdg-open`
   child is reaped on a thread of its own.
7. **Startup apps.** Turning an everyone entry back on removes the person's
   copy when it only differed in the switches; copies are written through
   `tempfile` with `sync_all` and renamed into place; an entry whose
   `TryExec` program is missing is shown as "Not installed".
8. **Services.** Only a plain `name.service` reaches `systemctl`, after
   `--`. A failed switch is the kit's `ErrorBar`: one plain sentence, with
   what `systemctl` said under "Show details"; no enum name or `systemctl`
   text is ever the message.
9. **Keyboard context menu.** The Menu key and Shift+F10 open a table row's
   menu under that row (`list::menu_anchor`), not at a fixed point.
10. **Compact navigation at 960** shows ☰ and names each item in a
    tooltip, now confirmed by screenshot.
11. The test that set the `systemctl` guard with `unsafe { set_var }` now
    passes the guard to `RealSystemctl` directly.

`monitor-gui` now depends on the workspace's `tempfile`; `Cargo.lock` gained
that edge and `docs/third-party-licenses.md` was regenerated.

### Not verified after the review

- "End process tree" refused on a real session leader was not driven in the
  window: in a shared machine the rows reorder live and a blind click lands on
  someone's real process. The rule is tested in `monitor-core` and against a
  fake `/proc` in `monitor-actions-linux`.
- The pidfd fallback for kernels before 5.3 was not run (this kernel has
  pidfds); the keyboard menu position was tested as arithmetic, not pressed.

### Screenshots after the review

In the same scratch directory: `review-en-dark-960-compact-nav-tooltip`,
`review-en-dark-960-hamburger-tooltip`, `review-en-dark-960-details-live-end-enabled`,
`review-en-dark-960-details-paused-end-disabled`,
`review-en-dark-960-startup-not-installed`,
`review-en-dark-960-service-switch-failed` and `-details` (the fake
`systemctl` recorded `--user disable -- pipewire.service` and answered with an
error), `review-en-dark-960-history-on-the-clock`, and
`review-en-dark-960-slow-moments-machine-share`. The 80 matrix screenshots
were taken before the review fixes.
