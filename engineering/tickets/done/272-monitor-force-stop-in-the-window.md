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

## Owner's decision (2026-11-22)

On the processes the desktop stands on, End task is not offered at all,
the same as Force stop. One protected-set rule governs End task, End process
tree and Force stop; the item is disabled with its reason everywhere it
appears, and the controller refuses again when run.

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
- Review round (2026-11-22): the first guard refused every session leader,
  which is every systemd user service and every terminal shell. It is
  replaced by the protected set in `monitor_core::session` (Better Monitor
  and its ancestors and its service, the user's service manager, the
  session bus's and the Wayland display's socket holders, the X server
  unless mutter runs it on demand, logind's display-session leader and what
  it started, the session managers by name), read by
  `monitor-actions-linux` from `/proc/net/unix`, descriptors, cgroups and
  logind's records, with names only in `session::NAMED`. The spec's
  "Nothing the desktop stands on" bullet has the rule and its sources.
  Run against this desktop read-only
  (`print_what_the_guard_decides_on_this_desktop`, ignored by default):
  gnome-shell, dbus-daemon, systemd --user, gdm-wayland-session, both
  gnome-session-binary and better-monitor-service refused; foot, bash,
  tracker, gvfsd, evolution, portals, pipewire and a nested gnome-shell
  offered.
- Per the owner's 2026-11-22 decision the same rule governs End task and End
  process tree; leading a session no longer refuses a tree. An application
  holding a protected process is refused whole for both actions (the spec
  says why).
- Every end command and menu item asks `MonitorApp::end_refusal` and shows
  the reason as its tooltip when disabled (`better-ui` gained
  `Command::reason` and `MenuItem::reason`). Confirmations count only what
  will be signalled; a partial end reports both counts and the reason, for
  End task too.
- Xwayland on GNOME 46 (Zorin 18, Ubuntu 24.04): on demand when gnome-shell
  runs in a systemd user unit (mutter 46.2 `meta-context-main.c`,
  `meta-xwayland.c`); seen here with no Xwayland running and gnome-shell
  holding both X sockets. It is offered there; under any other compositor
  it is protected. Flatpak: `flatpak run` 1.14.6 passes no `--new-session`,
  and a Flatpak application may be ended.
- Found on the way: a Suspend or Resume used to report "Asked it to close.";
  the outcome sentence now follows the signal (`i18n::signal_sentence`).
- Seen in a headless sway (wayhand sandbox) with the window on a private bus
  that activates nothing and a scratch home, on stand-in processes the run
  started: an application of three processes in an app scope, a stand-in
  user service (its own unit and session), a terminal shell (bash leading
  its own session), and as protected stand-ins the sandbox's private bus
  (message bus) and the `dbus-run-session` the window runs under. First
  round in `better-os-evidence/272`, this round in
  `better-os-evidence/272/round2`, light and dark, en-US and zh-TW.
- Second review round (2026-11-22): the facts are read off the window's
  interface thread (one pass over this person's descriptors for every
  socket, logind over D-Bus instead of its private files) and kept in the
  model; drawing answers from them only (`snapshot_view`, tested with a
  reader that panics), they are read again when a holder is gone or an
  answer met a process they do not cover, and `perform` reads fresh facts
  before every signal. A process whose descriptors cannot be read is
  protected by its name. Another graphical session of the person (COSMIC
  beside GNOME) has its compositor, X server, leader and session manager
  protected by the same facts. Timing: one read is 22.7 / 25.9 / 30.5 ms
  (fastest / median / slowest of ten, release build, 212 processes of the
  person, 205 with readable descriptors), from
  `print_what_the_guard_decides_on_this_desktop`.
- A disabled item's reason now shows beside the menu, or under the command
  bar's button, never over another item, and is the item's accessible
  description (`role`, `aria_label`, `aria_description` in the pinned
  GPUI). The pinned GPUI has no disabled state for accesskit, so a screen
  reader is not told the item is disabled; not checked with a screen
  reader, because a sandbox on a private bus has no accessibility bus.
  Screenshots in `better-os-evidence/272/round3`.
- Seen here, read-only: logind answers `User.Display` `("2", …/session/_32)`,
  session 2 `Leader` 2321, `Type` wayland, `Class` user, and
  `GetSessionByPID` fails for a process outside any session (the window
  then takes the display session as its own). Whether GDM lets one person
  hold a GNOME and a COSMIC session at once is not checked (no VM here); the
  rule is tested with a fixture of both.
- Not seen: the guard against a real GNOME or COSMIC session's own
  processes in a window (only read-only, by the ignored test), and a
  COSMIC session at all (fixture trees only).
