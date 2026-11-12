# Ticket 89 — Control center: Default apps

**Blocked by:** 84 · **Status:** ready

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

- [ ] Each Better Defaults row of spec section 12 is shown working; every
      existing defaults test still passes.
- [ ] Light and dark, zh-TW and English, 1600 and 960, screenshotted, against
      a fixture `mimeapps.list` and dconf in the scratch HOME, never the
      person's own.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
