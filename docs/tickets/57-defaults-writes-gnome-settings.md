# Ticket 57 — Better Defaults applies a GNOME shortcut or setting itself

**Epic:** Better Defaults (Issue #10) · **User Story:** a person who applies a
component's GNOME shortcut or setting sees it applied, not "Manual action
required" · **Branch:** `ticket-57-58` · **Blocked by:** none ·
**Status:** ready

## What it delivers

`DconfAdapter::write` always returns Manual action required. Ticket 29 built a
dconf write path in `touchpad-platform` — `ca.desrt.dconf.Writer.Change` over
the session bus, with the change set encoded to GLib's bytes — and ADR 0010
records it as the way a GNOME setting is written. `touchpad-platform` already
depends on `defaults-platform`, so the writer moves down into
`defaults-platform` behind an optional feature and `touchpad-platform`
re-exports it. The GNOME defaults adapters then write, read back, and report
what the read saw.

## Acceptance criteria

- [ ] The writer and the GVariant encoder live in `defaults-platform`;
      `touchpad-platform` keeps its public names by re-exporting them and its
      tests pass unchanged.
- [ ] The encoder supports string arrays (`as`), pinned against bytes GLib
      itself produces, like the existing encodings.
- [ ] Apply writes, re-reads, and reports Applied only when the read matches.
      A mismatch or a failed write reports the failure and changes nothing
      else.
- [ ] Restore of a key that had no user value resets the key rather than
      writing the default.
- [ ] Tests use a private session bus or a fake writer. No test writes the
      user's real dconf database.
- [ ] ADR 0009's "not adopted" status is updated to say what changed.
