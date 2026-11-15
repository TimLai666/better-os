# Ticket 85 — Control center: Apps (installing, updating, checking components)

**Blocked by:** 84

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

## What was built

Spec section 7 changed first: the owner's update flow of 2026-11-15 (no
review page, changes start at once, per-row progress and results, errors on
the row with Retry and Restore, only Remove and restore-to-older ask) replaces
"keeps the review → installing → finished / restore → restored flow". The
spec also states the personal-data rule and the section 12 rows that moved.

- **Apps page** (`render_apps.rs`, rules in the GPUI-free `apps.rs`): the
  Updates (with count), Health, and Activity cards; "Better OS components"
  with the `SelectorBar` filter (All · Installed · Updates · Turned off ·
  Needs attention), "Search components", and the catalog line `{age} ·
  Refresh list` with its warning and refused-entries line. Each component is
  a `SettingsCard` with one thing at its edge (`apps::trailing`): a state word,
  one action (Install, Update, Turn on), View for drift or an unorderable
  version, live progress, the finished change's result, or, for a failure,
  the reason as its description with Retry and Restore previous version
  (audit M2, M9). Every first-party component now has its line in both
  languages (audit M3; `apps::first_party`).
- **Component page**: breadcrumb `Apps › {name}`, Overview · Versions
  selector (audit M8), drift `InfoBar` with Adopt (confirmed in a
  `ContentDialog`), unorderable-version bar, last failure as an `ErrorBar`
  (stage, evidence key, service detail, recovery under Show details) with
  Retry/Restore, the header card with the one action, what it does, status,
  type, replaces, enhances, afterwards (only while an update waits), health
  checks, files touched, the Default apps link when it declares defaults,
  versions and release notes, and Manage: Turn off/on, Check again, Restore
  previous version, Remove, or the cannot-remove-self card for Better
  Manager. Only actions the planner would accept are offered.
- **Updates**: the check result card (Check for updates, and the one accent
  button Update all only when something waits, audit M4), the list's warning,
  the self-update notice (reworded: it reopens by itself, ticket 105), and one
  expander per update with versions, download size, afterwards, whether the
  previous version is kept, and release notes. Update all and every row action
  start at once as one transaction; a running card shows its title, stage, the
  password note, and Cancel while cancelling is honest; rows show
  per-component progress (`apps::row_progress`: plan-order downloads with
  bytes, then indeterminate stages) and then "Updated to 0.2.15". A real
  change a closed window left part way gets Continue. Demo mode steps itself
  a stage a second.
- **Health**: interrupted-package `InfoBar` with Repair (accent while
  needed), the repair result, the summary with "Run the full check" (re-reads
  dpkg and the record, says how long ago), and every result as a list row with
  View on a failed component.
- **Activity**: filters, list with status glyphs and age, Clear history
  behind "Clear all activity?".
- **Remove** asks with "Also delete my settings and history", ticked by
  default, shown only when the component owns entries on the closed list of
  Better OS data directories (`manager_core::personal_data::DATA_DIRECTORIES`,
  owner 2026-11-15, built from every manifest, every crate's path constants,
  and `git log -p --all -S`). Removing a component deletes only its own
  entries, historical ones included, never another component's, the
  manager's state, `mimeapps.list`, or GTK bookmarks; a manifest home path
  that is not its own entry is refused and logged (`refused_manifest_paths`). After a successful removal the data is moved to the trash
  through `files-platform`'s trash (`manager_platform::personal_data`, feature
  `personal-data`), never unlinked; the result shows as an InfoBar.
  `better-manager-cli run <id> remove --keep-data` keeps it; a mock run names
  what it would move.
- **Launch check obeys "Check for updates when opened"**
  (`apps::refreshes_at_launch`); `BETTER_MANAGER_OFFLINE` unchanged.
- The old review, installing, finished, restore, restored, and doctor-results
  pages are gone (`pages_main.rs`, `pages_flow.rs`, `pages_settings.rs`);
  a restart hand-off naming them opens Updates or Health. Back and the Apps
  item no longer jump to a progress screen. The first-run page was already
  gone (ticket 84). Apps is `PageStatus::Built`; its search entries now also
  find the component list and Refresh list; the nav badge is the update count.
- `BETTER_MANAGER_DEMO_DPKG_DIR` lets a demo window read a fixture dpkg
  directory, for screenshots; a real window never reads it.

### Audit findings

M2, M3, M4, M8, M9 closed here (above). M5, M6, M7, M12 were closed by
ticket 84. M10 (dark cards darker than the window) no longer applies: every
card is the kit's `SettingsCard` on the kit's window background. From the
owner's wording:

- **M1** (Settings choices did not show which is selected): Appearance and
  Language are kit `ComboBox`es showing the current value, and the open list
  marks the selected item with the accent bar (screenshots 26, 28); Update
  channel is read-only text. Closed by ticket 84's Settings, checked here.
- **M11** (button text ~18px, bolder than body): every button is the kit's
  `Button`, Body 14px regular, the same as row text (27, 28). Closed.
- **M13** (Awake and Git shared the inbox icon): their manifests can only say
  `generic` (the icon set is closed and a released manager refuses a new
  value), so the shipped first-party table now gives Awake a sun, Git the
  GitHub mark, and Storage a drive (`components::first_party_glyph`); a test
  asserts no two built-in components share a glyph (27). Fixed.
- **M14** (switches white when on): the kit's `ToggleSwitch` fills accent
  when on (26, 28). Closed.

### The click bug (found in review)

Buttons on the right half and bottom of Better Manager ignored the pointer;
cards on the left worked. Cause, read from the running window: the first
frame was client-decorated at 1280×820 and set the Wayland input region to
that frame (`52,52 1176×716`). sway then drew the decorations itself and the
window grew to 1600×1000, and `FramedView::render`'s early return for a
server-decorated window never cleared the region, so the compositor dropped
every press outside the first frame. Fixed at the source in
`better_ui::fluent::window_frame`: losing client decorations sets the whole
surface again (`system_frame::input_region_changes`, test
`a_window_that_loses_client_decorations_takes_input_everywhere_again`). It
affected every Better OS window in that situation, since they share the
frame. Proved with real pointer events in a private headless sway: Update
all (21), a row's Update (22, the card did not also open), Remove confirmed
with the box ticked (23, 24: row reads 已移除), Restore confirmed (25:
已還原上一個版本), and the Appearance list opened (28).

### Checks and screenshots

See the commit message for the commands. Screenshots (own headless sway via
wayhand, scratch HOME, demo mode, fixture state with two updates, a failed
Better Git install with a restore point, Better Launcher drift, and a fixture
dpkg directory with an unfinished package) are in the session scratchpad
`shots-85/`: 01–09 zh dark 1600 (Apps, Updates, running, later stage,
results, Health, Activity, failed component, drift), 10–13 en light 1600,
14–17 at 960 in both languages and themes, 18–20 the Restore and Remove
dialogs, 21–25 pointer clicks after the fix, 26–28 M1/M11/M13/M14.

### Not verified

- The click fix was proved under sway; it was not tried on GNOME/Mutter,
  which never negotiates server-side decorations, so it never hit the bug.
- The trash move itself was not done on screen (demo mode changes nothing);
  the rule, the move, and the CLI are tested.
- No real transaction was run (demo only, by the ticket's rule).

Gates: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings` on stable and `+1.99.0`; `cargo +1.95
check --workspace --all-targets --locked`; `generate-third-party-notices.sh
--check` (regenerated: `manager-platform` gained an optional edge to
`files-platform`); the manager crates' tests, 605 passed. The workspace run's
only failures were inotify tests in app-catalog-platform, awake-platform,
better-core, and files-gui ("Too many open files": the host's 128 inotify
instances were in use by other sessions), crates this change does not touch.
