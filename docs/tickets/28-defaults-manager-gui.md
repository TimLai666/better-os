# 28 — Manager Defaults GUI: summary, review flows, preview-and-apply

**Epic:** Better Defaults (Issue #10)
**User Story:** A user opens Defaults, sees exactly which Better OS components
are the system default, and can change or restore them — one at a time or
together — always after seeing what will change.
**Blocked by:** 27-defaults-core-and-adapters
**Status:** todo

## Goal

Put Issue #10's confirmed UX on screen: preview before applying, with no button
anywhere that silently replaces every default.

## What it delivers

- A first-class **Defaults** section in Better Manager's navigation, and a
  compact Overview summary line of the form Issue #10 shows — how many
  components are defaults, how many can be changed, how many were changed
  outside Better Manager.
- The Defaults screen: the two top-level actions **Use Better OS defaults** and
  **Restore previous defaults**, both opening a review screen before anything
  changes, above one row or card per component showing component name and icon,
  the default integration types it supports, the current default owner, the
  Better OS target owner, current status, last verified time, whether
  restoration data exists, and its individual actions.
- Component detail listing every declared integration separately with its
  effective owner or value, so a partial state is never hidden behind one label.
- Individual actions: Make default, Review changes, Restore previous default
  when a valid snapshot exists, and Verify again. A fully default component
  shows a non-destructive status rather than a meaningless enabled switch. An
  individual operation touches only that component's declared integrations.
- Global review screen for Use Better OS defaults: every eligible installed and
  healthy component, independently selectable, selected by default but
  uncheckable. Per proposed change it shows the component, the integration, the
  current value or owner, the new value or owner, whether sign-out or restart is
  required, whether the previous value can be restored, compatibility warnings,
  and conflicts or settings that cannot be changed automatically. The bottom
  summary carries components selected, settings affected, sign-out or restart
  requirements, snapshot status, and anything needing manual action. Primary
  action Apply selected defaults; secondary Cancel.
- Restore-all review screen showing the exact saved value for every integration
  and distinguishing safe to restore, already restored, changed externally since
  the snapshot, previous target no longer exists, and restoration requires manual
  action. The user restores all safe entries or selects individual components,
  and a Changed externally entry needs its own explicit confirmation.
- Per-entry results after apply or restore: immediate, sign-out required,
  partial success, failed, restored, and manual action.
- The preview states plainly when elevated access will be requested, before it is.
- No backend vocabulary in the UI. No `commit transaction`.
- `zh-TW`, `en-US`, and system language with runtime switching, using Issue #10's
  fixed Traditional Chinese terms: `預設值`, `設為預設`, `套用 Better OS 預設值`,
  `恢復先前的預設值`, `部分為預設`, `已由其他程式變更`, `需要登出後生效`.
  Content-sized buttons and tags, wrapping descriptions, responsive action
  groups, at 100%, 125%, and 150% scaling.

## Out of scope

- The defaults engine, adapters, snapshots, and CLI (ticket 27).
- Any path that makes a component default during installation or update.
- A privileged executor for system-wide defaults.

## Deferred decisions

Issue #10's deferred list is carried by ticket 27 and constrains this screen too:
which Better OS integrations are enabled in the initial catalog, and whether
related integrations are grouped into one user-facing toggle, are not settled by
the row layout. Present integrations individually until that decision is
recorded.

## Acceptance criteria

- [ ] Better Manager contains a dedicated Defaults section in its navigation.
- [ ] Each component row shows Default, Not default, Partially default, Changed
      externally, Unavailable, Conflict, Unknown, or Needs sign-out as
      appropriate.
- [ ] Opening a component shows every declared integration with its effective
      owner or value.
- [ ] A user can review and make one component default without changing
      unrelated components.
- [ ] A user can review and restore the previous default for one component.
- [ ] The global apply action always shows a selectable preview before any
      mutation, and no UI path applies every default without one.
- [ ] The global restore action shows the exact captured values before mutation.
- [ ] A Changed externally entry requires explicit confirmation and is never
      overwritten by restore-all.
- [ ] Partial failures show exact per-integration outcomes.
- [ ] The preview states when elevated access will be requested, and cancelling
      before approval mutates nothing.
- [ ] The GUI executes no `gsettings`, `xdg-mime`, shell command, or privileged
      operation directly.
- [ ] GUI, CLI, and diagnostics share the same `defaults-core` logic.
- [ ] `zh-TW` and `en-US` layouts pass overflow tests at 100%, 125%, and 150%
      scaling.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `manager-gui` reaching the Defaults section
- A GUI test asserting the global apply path always produces a review model
  before a plan is executed
- A locale and scaling overflow test pass for `zh-TW` and `en-US` at 100/125/150%
- A test asserting `manager-gui`'s dependency surface reaches the platform
  adapters only through `defaults-core`
