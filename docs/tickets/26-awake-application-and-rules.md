# 26 — Better Awake full application and automatic trigger rules

**Epic:** Better Awake (Issue #13)
**User Story:** A user writes a rule that keeps the machine awake while a build
runs, sees every reason the machine is currently awake, and knows the battery
will still stop it at 20%.
**Blocked by:** 25-awake-tray-sessions
**Status:** todo

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

Issue #13 requires an ADR comparing viable options rather than a silent choice
for: the exact first production trigger provider set, whether fullscreen
detection requires a minimal GNOME adapter, lid-closed support policy, and rule
history retention duration. Providers ship behind a capability-reporting trait so
the production set stays a decision, not an accident of what was easy to write.

## Acceptance criteria

- [ ] All eight GPUI sections exist and are reachable by keyboard alone.
- [ ] Automatic rules can be created, edited, enabled, disabled, duplicated, and
      reordered, with AND/OR condition groups.
- [ ] A rule can be tested without acquiring an inhibitor.
- [ ] Automatic rules can be enabled, paused, and inspected from the tray.
- [ ] Multiple active trigger reasons are shown accurately, and ending one
      reason while another is active keeps the machine awake with an explanation.
- [ ] A provider unavailable on the current platform shows an explanation
      instead of an inert control.
- [ ] Low-battery protection ends a session safely and produces a notification
      and a history entry.
- [ ] History records the required fields and contains no sensitive command-line
      arguments by default.
- [ ] The tray and GUI never execute an arbitrary shell command from a user rule.
- [ ] Normal operation requires no root.
- [ ] Better Manager can install, verify, disable, and remove the component
      cleanly, and uninstall releases inhibitors and removes the tray
      registration.
- [ ] `zh-TW` and `en-US` layouts pass overflow tests at 100%, 125%, and 150%
      scaling.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `awake-gui`
- A rule-engine test suite covering AND/OR evaluation, priority, conflict
  explanation, reason merging, and each provider's unavailable path
- A battery-safety test driving the threshold from a fake power provider through
  to the stop, the notification, and the history entry
- Manifest validation through `better-core`, plus an uninstall smoke asserting
  inhibitors released and tray registration removed
- Idle CPU measured with no polling provider active, and provider polling
  intervals recorded
