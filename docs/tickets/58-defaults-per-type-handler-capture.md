# Ticket 58 — Better Defaults can capture and restore a mixed handler group

**Epic:** Better Defaults (Issue #10) · **User Story:** a person whose image
types open in two different viewers can still apply Better Files' handler
group and later get both viewers back · **Branch:** `ticket-57-58` ·
**Blocked by:** none · **Status:** merged, not released

## What it delivers

A handler group's previous value is stored as one owner. When the group's
types currently point at different applications, the value reads as unknown
and apply is refused. After this ticket the snapshot stores the previous owner
per declared type, apply is allowed after the preview lists each type's
current owner, and restore writes each type back to its own previous owner.

## Acceptance criteria

- [x] The snapshot stores per-type previous values. Snapshots written before
      this change still load and restore as before.
- [x] The preview in both the CLI and the GUI lists the per-type owners when
      they differ.
- [x] Restore writes each type to its own captured owner and reports per type
      what the verifying read saw.
- [x] A type that had no owner is reported as Manual action required on
      restore, as today, until a remove-association operation exists.

## What was built

### A reading per key

`better_core::ObservedValue` gained `Mixed { per_key }`, a list of
`KeyObservation { key, observed }` in declared order. `collapse` returns it when
every declared key was read definitely and they disagree. When a key could not
be read definitely, the whole reading stays unsupported, refused, or unknown as
before, because a mixed state with a hole in it cannot be put back. A mixed
reading counts as definite, and it has no single value, so it is never equal to
the value Better OS wants and never names a winner.

The engine needed almost nothing new for status and planning, because every
rule it had already works on a definite reading that is not the desired value.
A mixed group is Not default, apply is planned rather than refused, the capture
holds the mixed reading, and after an apply a mixed reading is Changed
externally, like any other value that is not the one Better Manager wrote.

### Restore goes key by key

`AdapterRequest::narrowed_to(key)` is the same request about one declared key.
Its `keys()` returns that key alone, so the XDG and dconf adapters needed no
separate per-key operation. A key the declaration no longer names gives nothing,
and the engine reports that key as Failed rather than writing a key the manifest
stopped claiming.

When the captured value is mixed, the engine writes each key back to its own
captured value and verifies each with its own read. The result is a new
`EntryOutcome::PerKey { keys }`, one `KeyOutcome` per type, each a normal
outcome: Restored with what the read saw, Manual action required, Not verified,
and so on. The entry succeeded only if every key did, and it counts as a failure
if any key does. A type that had no default is still Manual action required,
because `app-chooser-core` still has no typed way to remove an association.

The snapshot record after a per-key restore claims no value, whether every key
went back or only some did. Only when every key is back is it marked already
restored; otherwise the capture stays available, so the rest can be retried.
Keeping the old claim after a partial restore would make the next inspect report
Better Manager's own restore as somebody else's change.

### Snapshot schema version 2

`previous_value` now holds the mixed reading, and nothing else in the record
changed. The version went from 1 to 2 anyway, because a version 1 reader cannot
parse a per-key value. With the new version, an older build reports the file as
written by a newer Better OS and keeps it, rather than calling it corrupt.
Version 1 files are read unchanged. A test loads a version 1 file written the
way the previous build wrote it, and an engine test restores from one.

### The preview in both surfaces

- CLI: `defaults plan` prints `current, per type:` with one line per type, and
  `captured previous, per type:` on a restore plan. `defaults restore` prints
  each type's own outcome under the entry, and `inspect` does the same under the
  integration.
- GUI: `observed_label` lists `type: owner` pairs joined with `; `. Every
  place that shows a current or saved value (the component row, the review
  entry's current and new owner, and the saved value) therefore lists each type.
  A per-key result row is Restored when every type went back, and otherwise
  says some settings changed and some did not, with each type's own result in
  the detail. The headline counts a partly restored group as partial rather
  than as a failure.

The simulated desktop used by `--execution mock` reads and writes a narrowed
request key by key, so a mixed group behaves the same way there.

### Deliberately not done

- Apply is not per type. It writes every declared type to the component's one
  desired owner, which is what the declaration asks for. Only restore needs to
  know that the types differed.
- The plan's own `schema_version` stays 1. A plan can now carry a mixed value,
  but nothing reads a plan back or checks the version. It is serialized for
  diagnostics only.
- No new GUI layout. The per-type list goes into the existing value text, which
  wraps. A layout that puts each type on its own row would be a GPUI change, and
  it needs someone to look at it on a desktop.
- No shipped manifest declares a multi-type handler group. `better-files.yaml`
  declares `inode/directory` only, so the case in the user story is reachable
  through a manifest that declares one, as the tests do, and not from the
  built-in catalog.

### Not verified

- The GUI was not looked at with a mixed group on screen. The model is tested
  in both locales, and the wrapping of a long per-type list has only been
  reasoned about.
- A restore was not run against a real user's `mimeapps.list`. Every test uses
  a file in a temporary directory.

