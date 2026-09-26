# Ticket 55 — A job of a million items can be recorded without rewriting it

**Epic:** Better Files (Issue #6) · **User Story:** a person can copy a very
large tree and still resume it after Better Files restarts · **Branch:**
`ticket-53-55` · **Blocked by:** none · **Status:** ready

## What it delivers

A job record is rewritten whole on every persist: about 17.5 MB at 10,001
items, growing linearly and rewritten every 250 ms. After this ticket a job is
stored as a small header plus an append-only item journal, so a persist writes
only what changed.

## Acceptance criteria

- [ ] Item progress is appended, not rewritten. The header is rewritten
      atomically and stays small.
- [ ] Recovery replays the journal and tolerates a torn final line from a crash
      mid-append by ignoring that line.
- [ ] A record written by the current format is read and migrated on load.
- [ ] The journal is compacted when a job finishes or when it exceeds a fixed
      multiple of the live item count.
- [ ] A benchmark or test records the bytes written per persist at 10,001 and
      100,001 items, before and after.
