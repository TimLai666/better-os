# Ticket 71 — Restoring a default that had no owner clears it

**Epic:** Better Defaults (Issue #10) · **User Story:** a person who restores a
file type that had no default application before Better OS gets exactly that
back · **Branch:** `ticket-71-72` · **Blocked by:** none · **Status:** ready

## What it delivers

Restoring an XDG default that previously had no owner reports Manual action
required, because `app-chooser-core` can set an association and cannot remove
one, and no second `mimeapps.list` editor may be written. The owner decided on
2026-09-27 that `app-chooser-core` offers a typed "remove this association"
operation, and Better Defaults' restore uses it.

## Acceptance criteria

- [ ] `app-chooser-core` removes one MIME type's entry from the user's
      `mimeapps.list` `[Default Applications]` section, leaving every other
      line, comment, and section byte-for-byte as it was, and leaving a file
      with nothing left in it valid.
- [ ] The operation is typed (a MIME type, never a free-form key) and has the
      same verification and rollback record as setting an association.
- [ ] Better Defaults' restore of a type captured as having no owner uses it,
      verifies by reading again, and reports Restored.
- [ ] Tests use a temporary `XDG_CONFIG_HOME`.
