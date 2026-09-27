# Ticket 70 — A contributor can keep the build tree from filling the disk

**Epic:** Build and release · **User Story:** a contributor runs one command
that frees the build space no longer needed, without losing the build they are
using · **Branch:** `ticket-70` · **Blocked by:** none · **Status:** ready

## What it delivers

`target/` reached 158 GB during the v0.2.4 work and 143 GB again before tickets
50–62. The owner decided on 2026-09-27 on a cleanup someone runs:
`packaging/prune-target.sh`.

## Acceptance criteria

- [ ] With no options it reports what it would remove and how much, and removes
      nothing. `--apply` removes it.
- [ ] It removes incremental compilation caches, stale artifacts of other
      toolchains' target directories nested in `target/` (for example
      `target/msrv-check`), and release artifacts older than a given age, and
      keeps the current debug build usable.
- [ ] It accepts a target directory argument so it works on a worktree's own
      `target/`, and refuses a path that is not a cargo target directory
      (no `CACHEDIR.TAG` with cargo's signature).
- [ ] `--help` documents every option; `shellcheck` passes; a test drives it
      against a synthetic target directory.
