# Ticket 173 — Rules found with two homes on 2026-11-21

**Blocked by:** none

## Why

AGENTS.md: a rule two places need lives in one place both call. Workers on
tickets 134 and 146 found these and left them as out of scope:

- `git-platform` writes its own `.trashinfo` file (`crates/git-platform/src/
  patch.rs`), a second trash writer beside `files-platform`'s
  (`crates/files-platform/src/trash.rs`): the name escaping, the collision
  rule for an existing name, and the per-volume trash choice can differ.
- `monitor-gui`'s `startup.rs::for_desktops` matches a desktop entry's
  `OnlyShowIn`/`NotShowIn` itself, while `app-catalog-core` has
  `DesktopEnvironments` for the same question.
- `packaging/test-daemon-e2e.sh` reads `/etc/os-release` with `sed` inside the
  end-to-end container; it is a test script, so decide whether it should call
  the binary's own reading instead and record the answer.

## What to build

- One trash writer used by both; one desktop-entry visibility rule used by
  both; a grep test for each so a copy cannot come back, as tickets 138 and
  146 added for their rules.

## Done when

- The copies are gone, the tests exist, and behaviour is unchanged where both
  copies agreed; where they differed, the ticket's notes say which behaviour
  was kept and why.
