# Ticket 89 — Control center: Default apps

**Blocked by:** 84

## Why

Better Defaults already lives inside Better Manager as the Defaults sidebar
page. It becomes the Default apps page of the control center on Fluent
controls, with nothing lost from its review-before-write, capture, verify,
and restore behaviour (tickets 27, 28, 57, 58, 72).

## What to build

Spec section 8, and the Better Defaults rows of section 12, every one of them.

- Page in `manager-gui/src/pages/default_apps/`: summary line, command bar,
  one `SettingsExpander` per component with its integrations as rows and its
  overflow menu, the review sub-page with per-entry `CheckBox`es, overwrite
  confirmations, missing-application alternatives, and the results sub-page.
- `defaults_model.rs` keeps being the only place states, preselection, and
  outcomes are decided, and `ApprovedPlan` keeps being constructible only
  from a review. Move it under the page module with its tests unchanged in
  meaning.
- The link from a component's details page (ticket 85) to its defaults opens
  this page filtered to that component.
- Register search entries and the Home item (a default changed elsewhere).

## Acceptance

- [x] Each Better Defaults row of spec section 12 is shown working; every
      existing defaults test still passes.
- [x] Light and dark, zh-TW and English, 1600 and 960, screenshotted, against
      a fixture `mimeapps.list` and dconf in the scratch HOME, never the
      person's own.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## What was built

- `manager-gui/src/pages/default_apps/`: `model.rs` (moved from
  `defaults_model.rs`), `jobs.rs` (moved from `defaults_app.rs`, still the one
  place `ApprovedPlan` is executed), `tests.rs` (moved from
  `defaults_tests.rs`; the execute-site scan now walks subdirectories),
  `render.rs` (Fluent page), `copy.rs` (spec words the old copy lacked), and
  `mod.rs` (page registration, search entries, Home item).
- Page: title, description, summary line, command bar, one
  `SettingsExpander` per component with a row per integration (owner → target,
  last checked, saved value, effect, state) and an overflow menu that offers
  only actions that can happen. Review and Results are breadcrumb sub-pages
  with per-component and per-overwrite `CheckBox`es, missing-application
  alternatives, and the bottom summary. Apps › details › Default apps opens the
  page filtered to that component.
- Home: "A default was changed by another app", counted in settings. Home and
  the page re-read defaults each time they are shown.
- An update restart waits while a plan is written or a review is open.
- Fixed on the way: a first change said its previous value "cannot be put
  back" though the engine captures it; "1 minutes ago"; results named every
  row by component only (now by setting kind). The `Interim` page status and
  the legacy helpers only the old screens used are gone.

Screenshots: `.shots/{en,zh}-{light,dark}-{1600,960}-NN-*.png` in the worktree
(not committed), taken in a private headless GNOME Shell 46 with a fixture
`mimeapps.list` and dconf in a scratch HOME, clicked with a remote-desktop
pointer.

Not changed: the tool-entry and launcher-entry integrations read "Not
available on this system" because `defaults-core` has no production adapter
for them (AGENTS.md); that is the engine's state, not the page's.
