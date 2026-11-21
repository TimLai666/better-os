# Ticket 272 — Decide whether Better Monitor offers Force stop, and build the answer

**Blocked by:** the owner's answer

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

`ProcessAction::ForceStop` exists in the contract and the controller, and
ticket 23 lists it as delivered, but the window offers no way to use it: a
program that ignores "End task" can only be killed from a terminal. Ticket
81's list of what the window does leaves it out without saying why.

## What to decide

- Offer it (in the row's menu, with its own confirmation that says unsaved
  work is lost, going through `monitor-actions-linux` and its checks for a
  reused process id, the person's own processes only), as Windows Task
  Manager's "End process tree" and GNOME's "Kill" do; or leave it out and
  record why in ticket 23.

## Done when

- The owner's answer is recorded here and built: either the action with its
  tests and both languages' wording, or the note.
