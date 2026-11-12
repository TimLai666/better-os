# Ticket 85 — Control center: Apps (installing, updating, checking components)

**Blocked by:** 84 · **Status:** ready

## Why

Today's Better Manager pages (Overview, Components, details, Updates and its
review/install/restore flow, System health, Activity) become the Apps page of
the control center. The audit found the component row crowded with pills and
buttons (M2), the "Update all" button prominent with nothing to update (M4),
tabs that look like buttons and a back button in the wrong place (M8), status
words that look like filters (M9), and dark cards darker than the window
(M10).

## What to build

Spec section 7, and the Better Manager rows of section 12, every one of them.

- Apps page: the three sub-page cards (Updates, Health, Activity), the
  component list as settings cards with one state word or one action and a
  chevron, the `SelectorBar` filter, the search, the catalog line with
  Refresh list.
- Component details sub-page with the breadcrumb and the Overview / Versions
  selector bar, and every card and action listed in section 7, each present
  only when the `manager-core` planning API offers it. Keep the CLI and GUI
  on the same planning API.
- Updates, review, installing, finished, restore, restored: today's flow and
  copy on Fluent controls, one accent button per screen and none when there
  is nothing to install.
- Health: run the full check, results, the interrupted-package card with
  Repair, view recovery.
- Activity: filters, list, Clear history behind a confirmation dialog.
- Make "Check for updates when opened" true: `refresh_catalog_at_launch`
  (`app.rs:363`) refreshes only when the stored `check_updates` is on.
  `BETTER_MANAGER_OFFLINE` keeps working as today.
- Remove the old first-run page; the first-run info bar is in Home (ticket
  84) and the compatibility facts in System › About this PC (ticket 86 adds
  the expander; until then they stay on Home's banner).
- Register Apps' search entries and its nav badge (update count).

## Acceptance

- [ ] Each Better Manager row of spec section 12 is shown working by a
      screenshot or a test, and audit findings M1–M14 are each closed or say
      why they no longer apply.
- [ ] A test shows the launch refresh obeys `check_updates`.
- [ ] Light and dark, zh-TW and English, 1600 and 960, screenshotted and
      looked at, including a fixture state with an update, a failed install,
      drift, and an interrupted dpkg (from fixtures or the demo mode, never by
      changing the real machine).
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## Notes

The real daemon is reachable from the sandbox. Use demo mode or fixtures for
every state that would change packages.

## Uninstall removes everything (owner, 2026-11-13)

The uninstall confirmation carries a check box, ticked by default, "Also
delete my settings and history" / 「同時刪除我的設定和紀錄」. Uninstalling
removes everything the component added (see AGENTS.md); when the box is
ticked it also deletes the person's data the manifest's `paths` name under
`~/.local/state`, `~/.config` and `~/.cache`, moving it to the trash rather
than unlinking it. The CLI takes `--keep-data`. Tests with a scratch HOME.
