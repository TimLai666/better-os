# Ticket 71 — Restoring a default that had no owner clears it

**Epic:** Better Defaults (Issue #10) · **User Story:** a person who restores a
file type that had no default application before Better OS gets exactly that
back · **Branch:** `ticket-71-72` · **Blocked by:** none · **Status:** merged, not released

## What it delivers

Restoring an XDG default that previously had no owner reports Manual action
required, because `app-chooser-core` can set an association and cannot remove
one, and no second `mimeapps.list` editor may be written. The owner decided on
2026-09-27 that `app-chooser-core` offers a typed "remove this association"
operation, and Better Defaults' restore uses it.

## Acceptance criteria

- [x] `app-chooser-core` removes one MIME type's entry from the user's
      `mimeapps.list` `[Default Applications]` section, leaving every other
      line, comment, and section byte-for-byte as it was, and leaving a file
      with nothing left in it valid.
- [x] The operation is typed (a MIME type, never a free-form key) and has the
      same verification and rollback record as setting an association.
- [x] Better Defaults' restore of a type captured as having no owner uses it,
      verifies by reading again, and reports Restored.
- [x] Tests use a temporary `XDG_CONFIG_HOME`.

## What was built

### The removal lives in the editor that already sets a default

`MimeAppsFile::remove_default(&MimeType)` in
`crates/app-chooser-core/src/mimeapps.rs` takes out every line in
`[Default Applications]` whose key reads as that type, and nothing else. A key
is matched the way `associations()` reads it, so `Image/PNG = eog.desktop`
counts as `image/png`; what is removed is exactly what a reader would have seen
as the default. Every copy of a repeated key goes, because a later copy left
behind would become the default instead of leaving the type with none. The
group header stays, so a file whose only default was removed is left as
`[Default Applications]` and nothing else, which parses and takes a new default
like any other file. The same type in `[Added Associations]` or
`[Removed Associations]` is not touched.

The change is a new `DefaultChange::Removed { lines }`, each line kept with its
old index and its own terminator. Undo puts them back byte for byte, and refuses
without touching anything when the type has a default again, when a line would
no longer land inside `[Default Applications]`, or when a missing newline would
join two lines into one: the line before the insertion point has none (another
editor saved the file without its final newline), or the reinserted line had
none and would no longer be the last line (somebody appended after it). Undo
never adds a newline of its own.

### The same store, record, and order as setting one

`AssociationStore::remove_default(&MimeType)` writes the rollback record before
it writes the file, through the same path `set_default_id` uses. The record
gained `PreviousDefault::Removed { lines }`, and `applied_desktop_id` became an
`Option`, empty for a removal. A record written before this change still reads,
because a stored string reads as `Some`. The schema version stays 1: an older
build that meets a removal record cannot parse the new kind and reports it
unreadable rather than misreading it.

A repeated default is reported with the existing `DuplicateDefaultKey` warning.
Removing from a file that does not exist creates nothing, and its record is a
no-op. Restoring a no-op record now returns before the "Better OS created this
file" branch, so a file somebody else writes later is never deleted by it.

### Restore uses it

The `xdg-default-app` adapter in `crates/defaults-platform/src/xdg.rs` turns a
clear into `remove_default` for each declared type, where it used to return
`xdg.clearing_a_default_is_not_supported`. The engine needed no change: the
verifying read sees no default, which matches the captured *nothing set*, so the
outcome is Restored. A group restored type by type (ticket 58) gets the same
result for each type that had no owner.

### Tests

- `app-chooser-core`: removing one line, removing the last default, CRLF and a
  missing final newline, a repeated key, case and spacing in the key, other
  groups left alone, an undo that refuses after someone set the type again, the
  record written before a write that fails, a missing file, and a record from
  before the change.
- `defaults-platform`: apply then restore a type that had no default returns the
  file to the exact bytes it started with.
- `defaults-core`: a single type and a mixed group, each restored to Restored
  with the file byte-identical. `tests/production_xdg.rs` runs the production
  adapter session in a child copy of the test binary whose `HOME`,
  `XDG_CONFIG_HOME`, and `XDG_DATA_HOME` are all a temporary directory, so the
  wiring a real run uses is what is tested.

### Not verified

- No restore was run against a real user's `mimeapps.list`. Every test uses a
  temporary file.
- `set_default` still finds a key by its exact spelling, while reading and
  removal compare parsed types. Setting a type the file spells `Image/PNG` adds
  a second line instead of replacing the first. That predates this ticket and
  is left as it was.

### Known and not handled

- A write is not protected against another program changing `mimeapps.list`
  between the read and the rename. The rename also replaces a symbolic link
  named `mimeapps.list` with a regular file, and replaces a read-only file.
  Setting a default always worked this way; removal goes through the same
  path, so it inherits all three.

