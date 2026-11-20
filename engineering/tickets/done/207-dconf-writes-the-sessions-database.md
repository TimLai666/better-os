# Ticket 207 — Better Defaults writes a GSettings key into the database the session reads

**Fixes:** better-os-session.md S5 (the dconf finding)
**Blocked by:** none

## Problem

`defaults-platform` always writes `/ca/desrt/dconf/Writer/user` and reads
`~/.config/dconf/user` (`dconf_writer.rs`, `dconf.rs`; ADR 0038). In a session
whose `DCONF_PROFILE` puts another writable database first (the Better OS
session's `betteros`, COSMIC's `cosmic`), a key it writes lands in GNOME's
database and is hidden by any value the first database holds, and its
read-back reports GNOME's value, not the one the session sees.

## Expected

The adapter reads the profile the target session uses (`DCONF_PROFILE`, else
`/etc/dconf/profile/user`, else dconf's default) and writes to and reads from
its first `user-db`, through `/ca/desrt/dconf/Writer/<name>`, so a write is
seen where it is meant and GNOME's database is untouched by a write meant for
the session.

## Acceptance criteria

- [x] Profile parsing in the one place both the writer and the reader use,
      with tests for `user-db:user`, `user-db:betteros` then `user-db:user`, a
      `system-db` line, a missing profile, and an unreadable one (reported, not
      guessed).
- [x] Against a real `dconf-service` on a private bus with
      `DCONF_PROFILE=betteros`: a write lands in `betteros`, `user` is
      unchanged byte for byte, read-back agrees; with the default profile the
      behaviour is today's.
- [x] `touchpad-platform`, which shares the encoder, is checked to go through
      the same path.
- [x] fmt, clippy on stable, and the crates' tests pass.

## Notes

- `defaults_platform::dconf_profile` decides the database; `DconfAdapter`,
  `LauncherEntryAdapter`, `ToolEntryPointAdapter` (`for_session`) and
  `touchpad_platform::GnomeBackend` (`connect`, `read_only_session`,
  `session_database`) all go through it, and `DconfWriter::connect_session`
  takes the writer path it names. Tests: `dconf_profile.rs`'s unit tests over
  scratch profiles, and four private-bus tests at the end of
  `crates/defaults-core/tests/gnome_writes.rs`.
- Deliberate limits, written in the module: a first `service-db` (or a
  `user-db` on NFS, which dconf turns into one), `system-db`, or `file-db` is
  reported and not written; `system-db` values and locks are not read, as
  before. A process whose `XDG_CURRENT_DESKTOP` names COSMIC or `BetterOS` and
  has no `DCONF_PROFILE` is reported rather than given GNOME's database.
- That the session's processes really carry `DCONF_PROFILE=betteros` is the
  session launcher's job (ticket 203) and is not exercised here.
