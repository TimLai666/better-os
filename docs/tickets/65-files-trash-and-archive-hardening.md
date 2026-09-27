# Ticket 65 — The Trash view, job numbering, and zip reading hold up under bad conditions

**Epic:** Better Files (Issue #6) · **User Story:** a person whose network share
is unreachable, or who runs two Better Files windows, or who opens a hostile
zip, gets a working window and a clear error · **Branch:** `ticket-65` ·
**Blocked by:** none · **Status:** ready

## What it delivers

Three gaps ticket 53, 55, and 56 left open. The Trash view reads every mounted
filesystem's trash, network shares included, so an unreachable server can stall
the listing. Two processes submitting a job at the same instant can pick the
same job number, because nothing claims it on disk before the first write. And
a zip is read whole, central directory included, before its entry count is
checked against the extraction limit.

## Acceptance criteria

- [ ] The Trash view does not read the trash of a network or FUSE filesystem
      unless its trash directory is already known to be reachable, and a slow
      or unreachable filesystem cannot block the home trash from listing. The
      rule for which filesystems are skipped is written down with its reason.
- [ ] A job number is claimed atomically on disk before the job is accepted,
      so two engines on one store never share a number, and recovery ignores a
      claim that never became a record.
- [ ] A zip whose end-of-central-directory record declares more entries than
      the limit, or a central directory larger than a fixed bound, is refused
      before the central directory is read, naming the archive.
- [ ] Each gap has a test that fails before the change.
