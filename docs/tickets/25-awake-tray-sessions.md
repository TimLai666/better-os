# 25 — Better Awake tray-first manual sessions

**Epic:** Better Awake (Issue #13)
**User Story:** A user clicks the tray icon, keeps the computer awake for two
hours in one action, and the session survives closing the window and restarting
the tray.
**Blocked by:** none
**Status:** todo

## Goal

Deliver Issue #13's Phase 1: the tray is the product surface, and a background
user service — not the tray and not the GUI — owns the inhibitor.

## What it delivers

- `awake-core`: session, policy, and effective-state model. Each session records
  prevent system suspend, prevent idle handling, prevent display blanking,
  prevent automatic lock, battery stop threshold, end condition, and the reason
  shown to the user and the backend.
- `awake-service`: the user-session daemon that starts with the session, owns
  inhibitor tokens, merges active reasons into one effective policy, survives
  the tray menu and the full window closing, releases all inhibitors on clean
  shutdown, and persists enough state to explain an interrupted previous session.
- `awake-ipc`: the typed local protocol the tray and GUI speak. The tray never
  executes `systemd-inhibit`, `gsettings`, or any shell command.
- `awake-store`: preferences and session state persistence for the service.
- `awake-tray`: one StatusNotifierItem over the session D-Bus, evaluating `ksni`
  or an equivalent maintained Rust implementation. It detects whether a
  StatusNotifierWatcher or host is available, registers when supported, and
  never claims the icon is visible without verifying registration. A
  desktop-entry and keyboard-accessible fallback remains.
- Tray icon states with distinct icon or overlay plus tooltip, never color
  alone: inactive, active manual session, active trigger session, paused rules,
  attention required, unavailable.
- The inactive and active popup menus exactly as Issue #13 lays them out —
  including the active menu's live status summary with reason, remaining
  condition, start time, and the system-sleep / display-sleep / automatic-lock /
  battery-protection rows, and its Extend, Change, and End actions.
- Session kinds: indefinite, timed presets, and until a selected time. A timed
  session shows remaining duration; an indefinite one shows "until ended".
- Default policy: prevent system sleep, allow the display to turn off, allow the
  screen to lock. Disabling automatic lock or keeping the display continuously
  on requires explicit first-time confirmation with the security and power
  consequence stated.
- systemd-logind inhibitor backend behind an abstract backend trait with
  capability reporting. The service picks the narrowest appropriate backend and
  verifies the inhibitor stays active. Normal operation changes no global GNOME
  power setting and needs no root.
- Session state and backend-failure notifications, and a minimal GPUI Status
  window.
- `zh-TW` and `en-US` tray wording, using the Traditional Chinese strings Issue
  #13 fixes: `保持清醒`, `目前未保持清醒`, `開始一段工作階段`, `持續保持清醒`,
  `直到指定時間`, `允許螢幕關閉`, `低於 20% 電量時停止`, `延長工作階段`,
  `結束工作階段`, `自動規則`, `暫停自動規則`, `開啟 Better Awake…`.

## Out of scope

- The full GPUI application beyond the minimal Status window (ticket 26).
- The trigger rule engine and every provider (ticket 26).
- The XDG Desktop Portal backend, lid-closed mode, watched transfers and jobs,
  and richer GNOME presentation/fullscreen integration.
- "While an app is running" as a working session type; the menu entry belongs to
  the rule engine in ticket 26.

## Deferred decisions

Issue #13 requires an ADR comparing viable options rather than a silent choice
for: the exact Rust StatusNotifierItem crate, the exact local IPC protocol,
whether tray and service ship as one process or two, the exact icon artwork and
active-state animation, and the final default preset durations. Evaluating
`ksni` is the ADR's input, not its conclusion.

## Acceptance criteria

- [ ] Better Awake appears as a StatusNotifierItem on a supported Zorin/GNOME
      installation, and registration is verified rather than assumed.
- [ ] Clicking the tray icon opens the compact menu without opening the full
      window.
- [ ] The inactive menu starts indefinite and timed sessions in one action.
- [ ] The active menu shows reason, remaining condition, and the effective
      sleep, display, and lock policy.
- [ ] A session can be ended, extended, or changed from the tray.
- [ ] Closing the Status window does not end an active session.
- [ ] Restarting the tray client does not end a service-owned session.
- [ ] The default session prevents system sleep while allowing display sleep and
      automatic locking.
- [ ] Disabling automatic lock or keeping the display on shows a security
      warning the first time.
- [ ] An unsupported tray host or a missing inhibitor backend produces an
      explicit explanation, not a silent failure.
- [ ] The tray and service execute no shell command.
- [ ] Normal operation requires no root and stores no password.
- [ ] The service releases all inhibitors on clean shutdown, and a crashed
      client leaves no permanent hidden setting behind.
- [ ] `zh-TW` and `en-US` tray labels fit the supported menu host without
      clipping.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- A private session-bus integration test of the StatusNotifierItem registration
  and the typed IPC surface, running unprivileged in CI
- A service-restart test: start a session, restart the tray, confirm the session
  and its inhibitor survived
- An inhibitor verification test against a fake logind backend covering acquire,
  verify, lost-inhibitor, and release
- Idle CPU and memory measured separately for the tray process and the service
