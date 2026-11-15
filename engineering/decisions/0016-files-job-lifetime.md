# ADR 0016: A Better Files job lives as long as the session

## Status

Accepted by the project owner on 2026-09-27.

## Context

`files-operations` runs copy, move, trash, archive, and the other file
operations as durable jobs. A job is written to the state directory as a header
and an append-only item journal (ticket 55), so a window that crashes or is
closed leaves a record, and the next Better Files window reports the job as
interrupted and offers to resume it. Ticket 33 scoped persistence to exactly
that: surviving a restart of the window.

Issue #6 left two questions open. Should a job keep running after the person
logs out or the machine reboots, and where does the boundary of Better Copy, the
background copy service the engine was built to serve, sit? `AGENTS.md` carried
both as a follow-up.

## Decision

A Better Files job does not outlive the login session. The engine runs inside
the Better Files process, logging out ends that process, and nothing resumes a
job on its own at the next login or after a reboot. The record stays on disk,
so the next Better Files window still reports the job as interrupted and the
person can resume it from there, as after a crash.

Better Copy is not part of this decision. A job that keeps running with no
window open, or across a logout, belongs to a separate background service with
its own lifecycle, and that service is a future proposal rather than an
extension of the window's engine.

## Consequences

- The window stays the only process that runs a job, and there is no user
  service, no systemd unit, and no autostart entry for file operations.
- A long copy that is running when the person logs out stops. The next window
  reports it as interrupted, the same way it reports a job that was running
  when the window crashed.
- Resuming after a reboot is manual. The engine never starts writing without a
  window that shows what it is doing.
- A proposal for Better Copy starts from the job engine and its journal as they
  are. It needs its own decision on how a job moves from a window to the
  service, and what the service may do with no one watching.
