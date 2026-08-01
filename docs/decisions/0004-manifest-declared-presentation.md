# ADR 0004: Manifest-Declared Presentation and Restart Metadata

## Status

Accepted for Better Manager.

## Decision

A component manifest declares its own `summary`, `icon`, and `restart` scope.
The manager presents a component from those values, falling back to the
translation this build ships for a first-party component.

- `summary` is at most 120 characters so a component row keeps room for its
  version, state, and action.
- `icon` is a closed set (`manager`, `monitor`, `files`, `launcher`,
  `touchpad`, `generic`) and defaults to `generic`.
- `restart` is `none`, `application`, `logout`, or `reboot`. A manifest that
  omits it is reported as undeclared.

A transaction inherits the widest interruption its steps declare. An undeclared
step never lowers that to "no restart needed".

## Why

Issue #8 requires every component row to communicate an icon, a short purpose,
what the component replaces or enhances, and its restart or logout requirement.
None of that was manifest data before this change: the GUI matched on three
hardcoded component IDs for names, purposes, and icons, and dropped any
component missing from that list. `RestartRequirement` had one variant,
`NotDeclared`, so the screen was truthful but empty.

Deriving presentation from the manifest means a component this build has never
heard of still renders with its own identity instead of disappearing.

## Why a closed icon set

Manifests are untrusted input. A free-form icon string would let a manifest
point a presentation layer at an arbitrary asset. A closed set costs a core
change when a new first-party component needs a new glyph, which is the correct
place for that decision.

## Why undeclared is not "not required"

Treating a missing declaration as "no restart needed" would invent a promise the
catalog never made. The distinction is preserved everywhere, including in the
transaction-level summary.

## Deferred

Localizing manifest-declared text is out of scope. This build ships
translations for its own components; a third-party component is presented in
the language its manifest declares.
