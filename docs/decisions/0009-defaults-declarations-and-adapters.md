# ADR 0009: Better Defaults declarations, storage, and the first adapter set

## Status

Accepted for the Better Defaults core (ticket 27). The GUI (ticket 28) consumes
these decisions and does not revisit them.

Decision 4's second option — writing through the `ca.desrt.dconf` D-Bus service
— was built by ticket 29 and is recorded in
[ADR 0010](0010-touchpad-ranges-and-dconf-writes.md). Better Defaults has not
adopted it; its two GNOME adapters still report manual action required, because
adopting the path changes this component's behaviour and its tests.

## Context

Issue #10 lists seven decisions that must not be made silently, and ticket 27
repeats them. Four of them block the core implementation and are decided here.
The other three are restated at the end with what still has to happen before
they can be decided.

## Decisions

### 1. Manifest field names

`default_integrations` is a list. Each entry carries `id`, `kind`,
`exclusivity`, `target` (`desired` plus `keys`), `platforms`, `sessions`,
`apply_adapter`, `verify_adapter`, `restore_policy`, `privileges`,
`session_effect`, and `health_prerequisites`.

The alternative considered was the flatter shape sketched in Issue #10, where
`desired_owner`, `desired_action`, and `capability` are separate optional
top-level keys per kind. It was rejected because optional keys per kind cannot
be validated: a declaration that names a shortcut and a desired owner is
nonsense that a flat schema has to accept. Folding the value and the settings it
applies to into one `target` makes the kind-to-value rule checkable, and it is,
in `DefaultIntegration::validate`.

`kind`, `apply_adapter`, `verify_adapter`, `exclusivity`, `restore_policy`,
`privileges`, `session_effect`, and `health_prerequisites` are closed enums, for
the same reason `icon` is: an untrusted manifest must not be able to name an
arbitrary asset, command, or executable. Adding an adapter is a schema change,
which is the intended cost.

### 2. Storage format

Snapshots are JSON files in a directory, one file per snapshot, named by
snapshot id.

Issue #10 sketches the record in YAML. YAML was rejected as the on-disk format
because `manager-store` already keeps versioned local state as JSON with the
same schema-stamp, atomic-write, and preserve-a-newer-writer rules, and a second
serialization format in the same product is a second set of failure modes for no
gain. The sketch's field names are kept; only the encoding differs.

One file per snapshot rather than one file containing a history: a history in
one file has to be rewritten to append, and rewriting is exactly what must never
put the only known-good baseline at risk.

### 3. Snapshot retention

No automatic retention limit, and nothing is ever deleted by this
implementation.

A limit was considered and rejected for now because the only safe limit depends
on a decision that has not been made — whether a new baseline can be promoted
automatically (see below). Until then, discarding an old snapshot could discard
the only record of the value a restore has to return to. Snapshot records are
small; the cost of keeping them is far below the cost of losing one.

### 4. The first production adapter set

Two adapters ship, and their honest capability differs:

| Adapter | Read | Verify | Apply / restore |
| --- | --- | --- | --- |
| `xdg-default-app` | yes, from the user's `mimeapps.list` | yes | yes, one line per declared type |
| `xdg-effective-default` | yes | yes | refuses — it is read-only, and the schema refuses to accept it as an apply adapter |
| `gnome-keybinding` | yes, from the user's dconf database | yes | **manual action required** |
| `gnome-desktop-setting` | yes, from the user's dconf database | yes | **manual action required** |

The keybinding decision is the one worth recording. Three options were weighed:

1. **Write the dconf file directly.** Rejected. The dconf service owns
   `~/.config/dconf/user`, caches it, and rewrites it. A change written behind
   the service is ignored by the running session and overwritten on the next
   write it makes. This would be the "reports a change that never happened"
   failure ADR 0005 exists to prevent.
2. **Write through the `ca.desrt.dconf` D-Bus service.** Viable and correct, and
   the eventual answer. It needs a GVariant change-set serialized in GVariant
   encoding, a D-Bus client dependency in a crate that currently has none, and a
   live session bus to test against. It is a larger, separately reviewable
   change than this ticket, and shipping it untested against a real session
   would be worse than not shipping it.
3. **Read and verify fully, and report manual action required for a change.**
   Chosen. Issue #10 explicitly allows "Manual action required" over a guessed
   command, and the reason returned names the exact keys and why Better OS did
   not touch them.

Reading is a real GVDB parser over the user's database, tested against a fixture
`dconf compile` produced. A key the user's database does not hold is reported as
unknown rather than as a default, because the effective value then comes from the
compiled GSettings schema, which this adapter does not read.

Every other integration kind has no production adapter at all. That is not a
gap in the model: an integration whose adapter is absent is skipped with
`NoProductionAdapter` and executes nothing.

## Consequences

- A component can declare an integration of any of the nine kinds today, and the
  status, planning, snapshot, and verification paths all work for it through the
  mock adapter set. Only the change itself waits on a production adapter.
- Restoring an XDG default that previously had no owner at all reports manual
  action required, because `app-chooser-core` offers no typed operation for
  removing an association and a second `mimeapps.list` editor is not acceptable.
  Restoring to a previous owner, which is the common case, works.
- A handler group whose declared types currently disagree reads as unknown
  rather than as one of them, so a mixed state is never flattened and
  overwritten. Per-key capture for that case is not implemented.

## Still deferred

These cannot be decided yet and are not decided here:

- **Which Better OS integrations are enabled in the initial catalog.** The
  shipped manifests in `components/manifests/` declare none. Deciding needs the
  components that would own them to exist. The schema is proven against
  `crates/better-core/tests/fixtures/every-integration-kind.yaml` instead.
- **Whether related integrations are grouped into one user-facing toggle.** This
  is a question about the review screen, so it belongs with ticket 28. The model
  supports either: an integration may name several keys, and a component may
  declare several integrations.
- **What happens when the previous application has been uninstalled.** Restore
  currently writes the captured desktop entry whether or not it is still
  installed, and the verifying read reports what the system then says. Deciding
  properly needs the shared application catalog consulted at restore time, which
  is a behavior change, not a detail.
- **Whether a new baseline can be promoted automatically.** Today a new baseline
  is only ever created by an explicit apply of an integration nothing was
  captured for. Automatic promotion would let Better OS decide that somebody
  else's change is the new "previous value", which is the opposite of what
  external-change protection is for.
