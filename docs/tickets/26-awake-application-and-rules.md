# 26 — Better Awake full application and automatic trigger rules

**Epic:** Better Awake (Issue #13)
**User Story:** A user writes a rule that keeps the machine awake while a build
runs, sees every reason the machine is currently awake, and knows the battery
will still stop it at 20%.
**Blocked by:** 25-awake-tray-sessions
**Status:** done, except the Better Manager uninstall smoke and the idle-cost
measurement — see Verification below for exactly what ran and what did not.

## Goal

Deliver Issue #13's Phase 2 and Phase 3: the complete GPUI application and the
trigger rule engine, with the tray gaining the rule controls it advertises.

## What it delivers

- `awake-gui` with all eight sections: Status, Quick Sessions, Automatic Rules,
  Session Defaults, Battery & Safety, History, Diagnostics, Settings. The GUI
  edits configuration and renders state; it never holds an inhibitor itself.
- Status: current effective policy, all active reasons, session start time and
  remaining condition, inhibitor and backend health, battery protection state,
  and end/extend/modify actions.
- Quick Sessions: configure and reorder the tray presets, select the default
  session policy, restore defaults.
- Automatic Rules: create, edit, enable, disable, duplicate, and reorder rules;
  AND/OR condition groups; rule priority with conflict explanation; and testing
  a rule without acquiring an inhibitor.
- The trigger engine in `awake-core`, with Issue #13's initial provider list:
  application/process running, AC power connected or disconnected, battery
  percentage range, external display connected, fullscreen/presentation state
  where reliably detectable, audio playback, CPU utilization threshold, network
  throughput threshold, selected network/Wi-Fi/VPN/interface, time schedule, and
  watched file or directory activity. A provider unavailable on the current
  platform shows an explanation rather than an inert control.
- Active-reason merging: multiple active reasons combine into one effective
  policy, and the UI explains why the machine stays awake after one reason ends.
- Tray rule controls: enable and disable automatic rules, pause for 15 minutes,
  1 hour, or until resumed, show active rule names, end a manual session without
  disabling trigger sessions, and override all active rules only after explicit
  confirmation.
- Battery & Safety: the battery stop threshold enabled by default on
  battery-powered devices, a notification and history entry on a low-battery
  stop, and a warning when the service is quit while a session is active.
- History in `awake-store`: start and end time, manual or trigger origin, active
  reasons, effective inhibitor policy, stop reason, backend failures, and
  battery safety stops — with no sensitive command-line arguments or arbitrary
  process data recorded by default.
- Diagnostics: backend and adapter status, verification results, and the
  compatibility explanation when no tray host is available.
- Better Manager component manifest for `better-awake` declaring service and
  autostart integration, the tray protocol requirement and detected host,
  inhibitor capabilities, supported distributions and desktops, configuration
  and history paths, and health checks. Uninstall stops and releases inhibitors,
  disables autostart units, removes the tray registration, and preserves or
  removes user rules and history according to an explicit user choice.
- `zh-TW`, `en-US`, and system language with runtime switching in the full
  application.

## Out of scope

- Anything Issue #13 puts in Phase 4: the sandbox/Portal backend, lid-closed
  compatibility mode, watched transfers and download jobs as a session end
  condition, and richer GNOME presentation integration.
- The tray, service, IPC, and logind backend themselves (ticket 25).
- Arbitrary shell commands as a rule action or condition, in any form.

## Deferred decisions

All four are decided in
[ADR 0010](../decisions/0010-awake-trigger-providers-and-retention.md): the
first production provider set (nine of eleven ship), fullscreen detection (needs
a compositor adapter, so it reports unavailable rather than shipping an X11-only
path that fails on a Wayland default), lid-closed policy (not supported, and not
faked through a global `logind.conf` change), and history retention (a bounded
count of 500, because no duration can be picked honestly without usage data).

Providers ship behind a capability-reporting trait so the production set stays a
decision, not an accident of what was easy to write.

## Acceptance criteria

- [x] All eight GPUI sections exist and are reachable by keyboard alone.
      Each is a sidebar entry with its own `ctrl-N` binding, asserted in
      `awake-gui`'s tests. Focus order *within* a page is not verified; only
      that no section is reachable only through another.
- [x] Automatic rules can be created, edited, enabled, disabled, duplicated, and
      reordered, with AND/OR condition groups.
- [x] A rule can be tested without acquiring an inhibitor. `RuleSet::test_rule`
      takes `&self` and returns a value, so this is a property of the type
      rather than a promise about the call site; the service test also asserts
      the backend is never even asked.
- [x] Automatic rules can be enabled, paused, and inspected from the tray.
- [x] Multiple active trigger reasons are shown accurately, and ending one
      reason while another is active keeps the machine awake with an explanation.
- [x] A provider unavailable on the current platform shows an explanation
      instead of an inert control. Fullscreen is the live example.
- [x] Low-battery protection ends a session safely and produces a notification
      and a history entry. The notification is a stderr line from the service,
      not a desktop notification — see Not done.
- [x] History records the required fields and contains no sensitive command-line
      arguments by default. `/proc/<pid>/cmdline` is never read at all, and
      reasons are redacted on the way into the store.
- [x] The tray and GUI never execute an arbitrary shell command from a user rule.
      Enforced by there being no `Condition` variant able to carry one.
- [x] Normal operation requires no root.
- [ ] Better Manager can install, verify, disable, and remove the component
      cleanly, and uninstall releases inhibitors and removes the tray
      registration. **Not done.** The manifest is written and validated, and the
      uninstall behaviour is declared in its release notes, but no
      `better-awake` package is built by `packaging/build-deb.sh`, so nothing
      was installed or removed. The uninstall smoke did not run.
- [x] `zh-TW` and `en-US` layouts pass overflow tests at 100%, 125%, and 150%
      scaling.

## Verification

What actually ran, crate-scoped rather than workspace-wide, because a
workspace-wide run rebuilds the GPUI world for every command:

- `cargo fmt --all -- --check` — passed across the whole workspace.
- `cargo check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`
  for `awake-core`, `awake-ipc`, `awake-store`, `awake-platform`,
  `awake-service`, and `awake-tray` — all passed, zero warnings.
- `cargo check -p awake-gui --all-targets`, `cargo test -p awake-gui`, and
  `cargo clippy -p awake-gui --all-targets -- -D warnings` — all passed.
- `ZED_HEADLESS=1` launch smoke of `awake-gui` — the built binary stayed alive
  for 8 seconds and logged nothing.
- 401 tests in total: 95 core, 31 ipc, 41 store, 83 platform, 57 service
  (47 engine plus 10 manifest), 69 tray (58 plus 11 on a private session bus),
  25 gui.
- The rule-engine suite covers the AND/OR truth tables in both directions,
  priority ordering, conflict explanation, reason merging with manual sessions,
  pause and override semantics, and every provider's unavailable path.
- The battery-safety test drives a real `/sys/class/power_supply` fixture tree
  through the production providers to the stop, the reported stop a notification
  is raised from, and the history entry — no number is pushed in by hand.
- Manifest validation through `better-core` passed. The uninstall smoke did not
  run; there is no package to uninstall.

Not run, and not claimed:

- `cargo check --workspace` and `cargo test --workspace`. The awake crates and
  everything they touch were gated; the full workspace gate should run
  downstream after merge.
- Idle CPU with no polling provider active was **not measured**. The polling
  intervals are recorded — in `awake-platform::provider`, as a table with the
  reasoning beside each number, and a test asserts none is sub-second — but a
  recorded interval is not a measurement, and no harness ran one.

## Not done

- **The uninstall smoke.** `packaging/build-deb.sh` builds no `better-awake`
  package, so the manifest's artifact checksums are placeholders and the
  component is validated but not installable. Same position Better Launcher is
  in.
- **A desktop notification for a low-battery stop.** The service reports the
  stop and writes the history entry; the process that should raise a
  `org.freedesktop.Notifications` message is the tray, and it does not yet. The
  service prints the stop to stderr so it is never silent, which is a worse
  answer than a notification and a better one than nothing.
- **Idle CPU and memory measurement** for the service and tray processes.
- **Icon artwork** for the six indicator states, still outstanding from
  ticket 25.
- **Audio playback over Bluetooth or network sinks** is not detected. The ALSA
  provider cannot see them; this is recorded in `awake-platform::audio` and in
  ADR 0010 rather than presented as full coverage.
- **Per-provider re-sampling.** An edit re-reads every provider rather than only
  the ones the edited rule names. On a rule set small enough to fit a menu that
  is a handful of small file reads, so the simpler path was kept.
