# Ticket 72 — Restore says when the application it would put back is gone

**Epic:** Better Defaults (Issue #10) · **User Story:** a person whose previous
image viewer has been uninstalled is told so on restore and can choose another
· **Branch:** `ticket-71-72` · **Blocked by:** none · **Status:** implemented, not merged

## What it delivers

Restore writes the captured desktop entry whether or not that application is
still installed, and reports what the verifying read saw. The owner decided on
2026-09-27 that restore checks the shared application catalog first: a
previous target that no longer exists is its own reported class, nothing is
written for it, and the person can choose another application or clear the
default.

## Acceptance criteria

- [x] Restore looks the captured desktop entry up in the shared application
      catalog at restore time; a missing entry is reported as "the previous
      application is no longer installed", naming it, and nothing is written
      for that integration.
- [x] The review screen and the CLI show that class distinctly, and offer the
      catalog's applications for that type or clearing the default (ticket
      71's operation).
- [x] A group restored type by type (ticket 58) reports the missing class per
      type and restores the others.
- [x] Tests use a temporary catalog and `XDG_CONFIG_HOME`.

## What was built

### The catalog travels with the adapters

`defaults-platform` gained `applications.rs`. `InstalledApplications` answers
two questions: whether a desktop ID is installed, and which installed
applications declare every one of a set of types. `ApplicationCatalog`
answers them from the shared catalog. `ApplicationCatalog::for_session()` loads
it with `app-catalog-platform::load_catalog`, so the directories,
precedence, and `Hidden=` handling are the ones every other Better OS surface
uses, and nothing here parses a desktop entry. An application with
`NoDisplay=true` is still installed; only shown applications are offered as
alternatives.

`AdapterSet::with_applications` attaches a catalog, and the production
adapter set in `defaults-core/src/adapters.rs` attaches the session's. A
simulated desktop attaches none and checks nothing, because its applications
are not the machine's. `defaults-platform` now depends on
`app-catalog-platform`; that is the one new line in `Cargo.lock`, no external
package was added, and `cargo tree` shows no GPUI in `defaults-platform` or
`manager-cli`.

### Restore looks first

While planning a restore, the engine looks every captured desktop entry up.
A single entry whose application is gone becomes
`SkipReason::PreviousApplicationMissing { desktop_id, alternatives }`, so
nothing is written for it and its capture stays available. In a group
restored type by type, the missing type keeps its captured value and gets a
`PlanWarning::PreviousApplicationMissing`, the other types are restored, and
the missing one is reported per type. The engine checks again just before it
writes, so an application uninstalled between the review and the run is still
not written back.

### Choosing instead

`RestoreChoice` is either another installed application or `ClearDefault`.
It is carried in `Confirmations`, next to the existing external-change
confirmations, for the entry or for one type of a group, and the plan is built
again with it. A choice naming an application the catalog does not hold is
ignored. Once a choice is in the plan, the entry still carries the warning
with the missing application and what was chosen, so the review keeps
showing both. Clearing is ticket 71's removal.

### Both surfaces

- CLI: `defaults plan --restore` and `defaults restore` print "the previous
  application is no longer installed: <id>; nothing is written for it", the
  alternatives, and the exact `--restore-to component:integration[:type]=<id>`
  and `=none` to use. A choice already made is printed as "restoring <id>
  instead" or "clearing the default instead".
- GUI: the decisions live in `defaults_model`, which has no GPUI dependency.
  The restore class is its own `PreviousApplicationMissing`, also when only
  one type of a group is affected. Each missing application is a row that
  names it, with a button for each alternative and one for "Leave without a
  default", and the chosen one is drawn as the primary button. The result
  screen names the class and the application. All new text is in zh-TW and
  en.

### Tests

- `defaults-platform`: the catalog adapter, from fixture desktop entries.
- `defaults-core/tests/missing_application.rs`: a skipped single entry, the
  second check at run time, choosing another application, clearing, an
  invalid choice, an installed application restoring as before, and a group
  with one missing type, all over a temporary `mimeapps.list`.
- `defaults-core/tests/production_xdg.rs`: the production session in a child
  process whose `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, and
  `XDG_DATA_DIRS` are a temporary directory with real desktop entries. The
  previous application's entry is deleted and the restore writes nothing.
- `manager-cli` and `manager-gui`: the words, the choices, and both locales.

### Not verified

- The review screen was not looked at on a desktop. The model and its words
  are tested, and the buttons wrap in a flexible row, which has only been
  reasoned about.
- Each defaults job in the window opens a new adapter session, so each plan
  reads the application directories again. On a machine with a few hundred
  entries that has not been timed.

