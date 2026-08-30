# 25 — Better Awake tray-first manual sessions

**Epic:** Better Awake (Issue #13)
**User Story:** A user clicks the tray icon, keeps the computer awake for two
hours in one action, and the session survives closing the window and restarting
the tray.
**Blocked by:** none
**Status:** done (Phase 1 scope; see "What is not done yet")

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

### What Phase 1 implemented, and why it does not close the decisions

None of these are recorded as decided. Each is written down here so the ADR that
follows starts from what was built rather than from memory.

**StatusNotifierItem crate — `ksni` evaluated, not adopted.** Phase 1
implements `org.kde.StatusNotifierItem` and `com.canonical.dbusmenu` directly on
the zbus connection the tray already needs, because it must talk to
`org.kde.StatusNotifierWatcher` and to `awake-service` on the same session bus
regardless.

The evaluation itself is incomplete, and honestly so: **this environment has no
network access** (crates.io returned HTTP 403 through the proxy) and `ksni` is
not in the workspace lockfile or the local cargo cache, so its current version,
license, and maintenance status could **not** be verified here. What can be
stated from the code that exists:

- Adopting any tray crate is a new dependency, and AGENTS.md requires that to be
  a decision rather than a side effect of implementation.
- The product logic — the menu model in `awake-tray::menu`, the wording table in
  `awake-tray::labels`, the registration verification in `awake-tray::sni` — is
  independent of the crate. Only `dbusmenu.rs` and `item.rs`, roughly 300 lines,
  would be replaced by adopting one.
- What a crate would buy: the dbusmenu property and layout-revision bookkeeping,
  and icon pixmap handling that Phase 1 avoids by using icon names only.
- What it would cost: the tray's D-Bus connection would be owned by the crate
  rather than shared with the service client, and a second async runtime
  integration would have to be checked against the workspace's zbus/tokio
  feature choice.

The ADR must confirm `ksni`'s license against ADR 0003 and check its maintenance
in an environment that can reach crates.io. Until then nothing depends on it.

**Local IPC protocol — session D-Bus, JSON documents.** `awake-ipc` carries
typed requests, replies, and events as JSON documents over a session-bus
interface `org.betteros.Awake1` at `/org/betteros/Awake1`, the same
document-inside-a-typed-call shape ADR 0007 chose for the privileged daemon. A
unix socket was rejected because the tray is already on the session bus for the
watcher, so a socket would add a second transport, its own peer-credential
question, and its own lifecycle for nothing. This is what Phase 1 ships; the ADR
may still choose otherwise.

**Two processes, not one.** `better-awake-service` and `better-awake-tray` are
separate binaries, which is what makes "restarting the tray does not end the
session" testable rather than asserted.

**Icon artwork.** Six distinct icon *names* are used
(`better-awake-inactive`, `-active`, `-active-trigger`, `-paused`,
`-attention`, `-unavailable`). No artwork is drawn and no animation exists.

**Preset durations.** 15 / 30 / 60 / 120 minutes plus indefinite and until-time,
as Issue #13 draws them.

## Acceptance criteria

- [x] Better Awake appears as a StatusNotifierItem on a supported Zorin/GNOME
      installation, and registration is verified rather than assumed.
      *Verified against a fake watcher on a private session bus, including the
      accepted-but-not-listed and no-host cases. Not verified on a real GNOME
      desktop from this environment.*
- [x] Clicking the tray icon opens the compact menu without opening the full
      window. `Activate` and `SecondaryActivate` do nothing; `ItemIsMenu` is
      true, so the host draws the dbusmenu.
- [x] The inactive menu starts indefinite and timed sessions in one action.
- [x] The active menu shows reason, remaining condition, and the effective
      sleep, display, and lock policy.
- [x] A session can be ended, extended, or changed from the tray. Change and
      until-time open the Status window, which is where a picker and the
      security warning can be shown.
- [x] Closing the Status window does not end an active session. The window makes
      one query and holds nothing.
- [x] Restarting the tray client does not end a service-owned session.
- [x] The default session prevents system sleep while allowing display sleep and
      automatic locking.
- [x] Disabling automatic lock or keeping the display on shows a security
      warning the first time. The state machine refuses an unconfirmed request;
      the tray, which has nowhere to show a warning, opens the window on that
      refusal.
- [x] An unsupported tray host or a missing inhibitor backend produces an
      explicit explanation, not a silent failure. The tray exits with a printed
      reason and a remedy key; the menu disables Start and states why.
- [x] The tray and service execute no shell command. The only process the tray
      starts is the `awake-gui` binary by name, with no arguments.
- [x] Normal operation requires no root and stores no password. The logind
      inhibitor needs neither.
- [x] The service releases all inhibitors on clean shutdown, and a crashed
      client leaves no permanent hidden setting behind. Nothing global is ever
      written; the lock is a file descriptor that dies with the process.
- [x] `zh-TW` and `en-US` tray labels fit the supported menu host without
      clipping. Asserted as a character-count bound on every menu label, not
      measured against a real panel font.

## What is not done yet

- The battery provider. `AwakeEngine::report_battery` is the seam and the
  threshold is carried end to end, but nothing reads the battery in Phase 1;
  that arrives with the trigger providers in ticket 26.
- Automatic rules. The menu row states `自動規則: 尚未提供` and the
  `暫停自動規則` entry is disabled, rather than showing a switch that does
  nothing.
- Desktop entry, systemd user unit, and Better Manager manifest. Packaging is
  ticket 26's along with the full application.
- Icon artwork for the six states.
- Idle CPU and memory measurement for the two processes.

## Verification

Gates were run crate-scoped, because a workspace-wide run rebuilds the GPUI
world for every command. The full workspace gates run downstream after merge.

- `cargo fmt --all -- --check` — clean.
- `cargo check -p awake-core -p awake-ipc -p awake-store -p awake-service
  -p awake-tray --all-targets` — clean.
- `cargo test -p awake-core -p awake-ipc -p awake-store -p awake-service
  -p awake-tray` — 142 tests, all passing.
- `cargo clippy -p awake-core -p awake-ipc -p awake-store -p awake-service
  -p awake-tray --all-targets -- -D warnings` — clean.
- `cargo check -p awake-gui` — clean.
- A private session-bus integration test of StatusNotifierItem registration and
  the typed IPC surface, running unprivileged: 11 tests, all passing.
- A service-restart test: a session started through one client is still active,
  with the inhibitor still held, after that client is dropped and a new tray is
  built against the same service.
- Inhibitor verification against a fake logind backend covering acquire, verify,
  lost-inhibitor, and release.
- Idle CPU and memory for the tray and the service: **not measured.**
