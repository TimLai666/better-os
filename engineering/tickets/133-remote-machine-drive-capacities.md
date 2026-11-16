# Ticket 133 — A remote machine's drives each show their own capacity

**Blocked by:** none

## Why

The owner reported on 2026-11-16: a remote machine (opened over SFTP from the
tailnet list) that has several disks shows only one capacity in Better Files —
its system disk's — even though browsing inside it reaches its other disks.
GVfs reports the free space of the filesystem behind the path asked about, so
a single figure for "the machine" is the root filesystem's.

## What to build

- Decide and record (spec, ADR if it adds a remote probe) how Better Files
  learns a remote machine's mounted filesystems over the existing SFTP
  connection without running commands the person did not approve (e.g.
  `statvfs` per known mount point through the SFTP `statvfs@openssh.com`
  extension, mount points from `/proc/mounts` read over SFTP).
- Show the remote machine's disks under it in the sidebar (or on its "This
  PC"-like page) each with its own capacity bar, and the capacity shown for a
  folder is the filesystem that folder is on, not the machine's root.
- Missing or unreadable data is shown as missing, never as the root's figure.

## Done when

- Tests against a fake SFTP server with two filesystems; seen against a real
  remote machine on the tailnet with the owner's consent.
