# Ticket 54 — A copied folder keeps its hard links

**Epic:** Better Files (Issue #6) · **User Story:** a person copying a backup
tree that uses hard links does not get a copy several times its size ·
**Branch:** `ticket-53-55` · **Blocked by:** none · **Status:** ready

## What it delivers

Two paths inside one copy job that name the same inode become two independent
files at the destination. After this ticket, the second and later paths are
created as hard links to the first copied destination, when the destination
filesystem supports it.

## Acceptance criteria

- [ ] Within one job, files sharing a `(device, inode)` pair with a link count
      above one are copied once and linked afterwards.
- [ ] If `link(2)` fails (cross-device destination, filesystem without hard
      links, permission), the file is copied instead and the job records why.
- [ ] A move within one filesystem is a rename and is unaffected. A move across
      filesystems keeps links the same way a copy does.
- [ ] A job resumed after a restart does not link to a destination that no
      longer holds the first copy's content.
- [ ] The policy table that says hard links are not preserved is updated.
