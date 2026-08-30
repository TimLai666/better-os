# 27 — Better Defaults core: manifest declarations, snapshots, adapters, CLI

**Epic:** Better Defaults (Issue #10)
**User Story:** Better Manager can say whether each component is currently the
system default for the integrations its manifest declares, change one of them
without touching the others, and restore the exact value that was there before.
**Blocked by:** none
**Status:** done (core, adapters, snapshots, CLI; GUI is ticket 28)

## Goal

Build the whole defaults engine below the UI: manifest declarations, typed
snapshots, the adapter traits, the first real user-scope adapters, aggregate
status, and CLI equivalents. Issue #10 is explicit that installing a component
must never make it the default.

## What it delivers

- `default_integrations` extension to the component manifest. Every declaration
  identifies a stable integration id, integration kind, exclusivity rules, the
  target Better OS owner or value, supported platforms and sessions, a typed
  apply adapter, a typed read/verify adapter, a restoration policy, required
  privileges, whether the effect is immediate or needs sign-out or restart, and
  its health prerequisites. Manifests stay untrusted input and are validated
  before planning, like every other manifest group.
- Integration kinds representable from the start: default application or
  desktop-entry handler, MIME and URI handler groups, desktop launcher or
  overview entry point, global keyboard shortcut, input-method selection,
  autostart or session activation, user service activation, file-manager or
  system-tool entry point, and component-specific desktop setting. A kind with no
  production adapter reports **Manual action required** rather than guessing a
  command.
- `defaults-core`: declarations, per-integration status, aggregate status, and
  serializable apply and restore plans. Global and individual operations use the
  same planning and verification path.
- Aggregate states, all eight: Default, Not default, Partially default, Changed
  externally, Unavailable, Conflict, Unknown, Needs sign-out. The per-integration
  detail is always available underneath; the aggregate never hides a partial
  state.
- `defaults-platform`: the read, apply, verify, and restore adapter traits,
  returning typed current and effective values. Mock adapters for every declared
  integration kind, plus the first real XDG/GNOME user-scope adapters — an
  `xdg-default-app` adapter and a `gnome-keybinding` adapter.
- `defaults-store`: typed snapshots with `schema_version`, snapshot id, creation
  time, system identity, and per-entry previous, better, applied, and
  last-verified values plus restore state. Snapshot history is kept rather than
  overwriting the only known-good record. Restoring one component leaves
  unrelated entries valid. A corrupted or incomplete snapshot is reported, never
  silently ignored.
- External-change detection: the effective state is read again before applying or
  restoring, and a value that differs from what Better Manager last wrote or
  verified is marked Changed externally and never overwritten silently.
- Health prerequisites from `manager-core` feed the Unavailable state.
- CLI: `inspect`, `plan`, `apply`, `verify`, and `restore`, sharing the same
  `defaults-core` logic as the GUI.
- No GPUI, no `gsettings` or `xdg-mime` invocation, and no privileged operation
  anywhere above the adapter boundary. User-scope changes require no root.

## Out of scope

- The Manager Defaults GUI, its review flows, and its localized layouts
  (ticket 28).
- Making a component default during installation or update, in any code path.
- Resetting the desktop to factory defaults.
- Privileged system-wide defaults where a user-scoped integration suffices; a
  privileged executor for defaults is not part of this ticket.

## Deferred decisions

Issue #10 requires an ADR comparing viable options rather than a silent choice
for: the final manifest field names, the exact first production adapter set, the
storage format and snapshot retention limit, which Better OS integrations are
enabled in the initial catalog, whether related integrations are grouped into
one user-facing toggle, the behavior when the previous application has been
uninstalled, and whether a new baseline can be promoted automatically.

## Acceptance criteria

- [x] The manifest schema carries `default_integrations` with all eleven
      required declaration properties, and invalid declarations are rejected.
- [x] Every one of the eight aggregate states can be produced, and each has its
      own test.
- [x] Read, apply, verify, and restore adapter traits exist, with a mock adapter
      for every declared integration kind.
- [~] `xdg-default-app` and `gnome-keybinding` user-scope adapters read, apply,
      and verify real values. `xdg-default-app` does all three. `gnome-keybinding`
      reads and verifies real values out of the user's dconf database and returns
      Manual action required for a change, because the dconf service owns that
      file and a write behind it would be ignored or overwritten. Issue #10
      allows that outcome over a guessed command; ADR 0009 records the options
      and what the eventual write path is.
- [x] An integration kind with no production adapter reports Manual action
      required and executes nothing.
- [x] The previous value is captured before the first change to an integration.
- [x] Restore returns to the captured value, never to a hard-coded Zorin default
      and never to a guess about which built-in application was selected.
- [x] An external change is detected before apply and before restore, and is
      never overwritten silently.
- [x] Every applied or restored integration is verified afterwards, and partial
      failure produces exact per-entry results.
- [x] Restoring one component does not undo successful unrelated entries.
- [x] A corrupted or incomplete snapshot is reported rather than ignored.
- [x] Installing or updating a component does not make it default, proven by a
      test over the install path.
- [x] The CLI provides `inspect`, `plan`, `apply`, `verify`, and `restore`, and
      shares `defaults-core` with the GUI.
- [x] Normal user-scope changes require no root, and cancelling before approval
      mutates nothing.

## What was built, honestly

- `better-core::defaults` carries the declarations; `ComponentManifest::validate`
  rejects every malformed one. The shipped manifests in `components/manifests/`
  declare no integrations, because which ones the initial catalog enables is a
  deferred decision. The schema's coverage fixture declares all nine kinds.
- `defaults-core` owns status, aggregation, plans, execution, and verification.
  Global and single-component operations are the same call with a different
  `Selection`.
- `defaults-store` keeps a directory of JSON snapshots, appends rather than
  overwrites, and reports every unreadable, future-schema, or incomplete file.
- `defaults-platform` has the traits, an in-memory adapter for all nine kinds,
  and two real ones. Their exact capability is in ADR 0009; the summary is that
  the XDG adapter reads, writes, and verifies, and the dconf-backed adapters read
  and verify but report Manual action required for a change.
- `manager-cli` gained `defaults inspect | plan | apply | verify | restore`, with
  `--execution mock|real` matching the rest of the CLI. Mock execution keeps its
  simulated desktop in a file the caller names and says so when it is not given
  one.

Known limits, none of them silent at runtime:

- Restoring an XDG default that previously had no owner reports Manual action
  required. `app-chooser-core` has no typed operation for removing an
  association, and a second `mimeapps.list` editor is not acceptable.
- A handler group whose declared types currently point at different applications
  reads as unknown rather than as one of them. Per-key capture for that case is
  not implemented, so a mixed group is refused rather than flattened.
- The distribution the CLI reports comes from `MockPlatform`, which is what every
  other manager-cli command uses today. Real distribution detection is a
  manager-platform concern, not a defaults one.

## Verification

Run on the touched crates. The workspace-wide gate runs downstream of the merge.

- `cargo fmt --all -- --check`
- `cargo check` and `cargo test` for `better-core`, `defaults-core`,
  `defaults-store`, `defaults-platform`, `manager-cli`, `manager-core`
- `cargo clippy --all-targets -- -D warnings` for the same crates
- Snapshot round-trip tests: capture, apply, verify, restore, and compare the
  restored value with the captured one
- An external-change test matrix: never touched, applied and still ours, and
  moved behind Better Manager's back, plus the per-entry confirmation that is
  the only thing that lifts the hold
- CLI smoke over all five subcommands against a disposable snapshot store and a
  mock adapter set
- A manifest validation run through `better-core` against a fixture manifest
  declaring every integration kind
