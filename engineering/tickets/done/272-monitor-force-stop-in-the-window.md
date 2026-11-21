# Ticket 272 — Better Monitor offers Force stop

**Blocked by:** none

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

`ProcessAction::ForceStop` exists in the contract and the controller, and
ticket 23 lists it as delivered, but the window offers no way to use it: a
program that ignores "End task" can only be killed from a terminal. Ticket
81's list of what the window does leaves it out without saying why.

## Owner's decision (2026-11-21)

Offer it.

## What to build

- Force stop in the row's menu and beside End task, with its own
  confirmation that says unsaved work is lost, going through
  `monitor-actions-linux` and its checks (a reused process id is never
  signalled; the person's own processes only; never Better Monitor itself or
  the session's own processes), as Windows Task Manager's "End process tree"
  and GNOME's "Kill" do. Both languages; refused while the view is paused, as
  End task is.

## Done when

- Tests for the action, each refusal, and the confirmation's wording in both
  languages; seen in a sandbox against a process the test itself started.

## Notes

- Delivered: Force stop beside End task on the Processes and Details pages,
  in the process row menu and the application row menu (one process, or
  every process of an application; not a tree), with its own confirmation
  (Cancel is the default). `crate::ending` in `monitor-gui` decides and
  carries it out against any `ProcessController`.
- The session guard: `monitor_core::force_stop_refusal` (the same rule as
  `tree_refusal`) refuses an ancestor of Better Monitor, the user's
  `systemd`, or a session leader as `ActionRefusal::SessionProcess`;
  `LinuxProcessController::availability` applies it, so `perform` refuses it
  again. An application holding such a process is refused whole.
- Found on the way: a Suspend or Resume used to report "Asked it to close.";
  the outcome sentence now follows the signal (`i18n::signal_sentence`).
- Seen in a headless sway (wayhand sandbox) with the window on a private bus
  that activates nothing and a scratch home, filtered to fixture processes
  the run started (`bm272-*`, `sleep` under another name; one ignoring
  SIGTERM, one a session leader by `setsid`): End task left the stubborn one
  running, Force stop ended it, the session leader was refused with the
  reason, and both are not offered while paused. Screenshots in
  `better-os-evidence/272`, light and dark, en-US and zh-TW.
