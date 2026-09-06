# Ticket 48 — Affordance audit

**Branch:** `ticket-48` · **Blockers:** none · **Status:** implemented, not merged

## The rule

Anything that **looks** clickable must **be** clickable, and anything that is
not clickable must not look like a button.

The rule is written down as a convention in `crates/better-ui/src/lib.rs` on
`better_ui::Affordance`, which splits every rendered element into two closed
cases:

- **Action** — a `Button`, `Checkbox`, `Switch`, sidebar item, or a `div` with
  both a click handler and a hover state. It may use the theme's action colors
  as a fill. It always has a handler. A control that is momentarily unavailable
  is a *disabled action*, and must still name something the person could
  otherwise do.
- **Status** — `better_ui::StatusPill`, `better_ui::badge`, or plain text. No
  hover, no cursor change, no click handler, no saturated fill in an action
  color. Tinted and outlined instead, so a status stays colour-coded without
  borrowing a button's shape and weight.

## Why the toolkit's own chip could not satisfy it

`gpui_component::Tag` breaks the rule in both halves, and a caller cannot fix
it from outside:

- **Same fill as a button.** `TagVariant::bg` returns `cx.theme().danger`,
  `primary`, `success`, `warning`, `info`. `gpui_component::button::Button`
  paints the same tokens for the same variants
  (`crates/ui/src/button/button.rs:786` returns `cx.theme().danger`). A filled
  danger `Tag` and a `.danger()` `Button` are the *same* fill on the *same*
  rounded shape.
- **An unremovable hover.** `Tag`'s render ends with
  `.hover(|this| this.opacity(0.9))` before `.refine_style(&self.style)`, so a
  caller's style refinement touches the base style and never the hover style.

Every first-party status indicator therefore moved to
`better_ui::StatusPill`, which carries a `StatusTone` (Neutral / Info /
Success / Warning / Danger) and is drawn through `better_ui::badge` with
colors the window derives from its own theme. A tone becomes a **tint**:
foreground and border in the accent, background at 12% of it.

## The typed distinction

Two types make the rule assertable without opening a window:

| Type | Where | What it does |
| --- | --- | --- |
| `better_ui::Affordance` | `crates/better-ui/src/lib.rs` | `Action` \| `Status`. `StatusPill::AFFORDANCE` is a constant a test reads. |
| `better_ui::StatusPill` + `StatusTone` | `crates/better-ui/src/lib.rs` | A status as pure data — label and tone, no colors, no GPUI state. |

Every status mapper in `manager-gui` is now an associated function taking a
`Locale` and returning a `StatusPill`, so a test can enumerate all of them.
`PrimaryAction::affordance()` in `defaults_model.rs` classifies the Defaults
row's leading control the same way, and the render reads it.

## Better Manager — page by page

Screens audited: overview, components list, component detail (all three tabs),
defaults list, defaults component, defaults review, defaults results, updates,
health, doctor results, activity, settings, first run, review changes,
installing, finished, restore, restored.

### Fixed

| # | Where | What it was | Call |
| --- | --- | --- | --- |
| 1 | Every screen listed above | 40+ status indicators built from `gpui_component::Tag`: filled `danger`/`success`/`warning`/`info`/`primary` pills carrying an unremovable hover, sitting beside buttons of the same fill | **Stripped the chrome.** All are `StatusPill` now. Colour-coding kept as a tint. |
| 2 | `components.rs` failure card | `key_value_row(c.restore_available, recovery_detail)` where `recovery_detail` defaults to `c.restore_available` — the row printed **可還原上一個版本 · 可還原上一個版本**, the same sentence on both sides of the colon | **Rewrote the row.** New key `recovery_status` ("Recovery" / "復原狀態") labels it; the value stays the recovery sentence. |
| 3 | `pages_defaults.rs` defaults row | `PrimaryAction::AlreadyDefault` drew a permanently `.disabled(true)` button whose label repeated the state pill beside it | **Stripped it.** It is not an unavailable action — there is no operation it could carry out. `PrimaryAction::affordance()` now says so and the row draws no primary control. The pill says it. |
| 4 | `app-chooser-gui/chooser.rs` | `badge_style(strong: true)` filled a `rounded_full` badge with `cx.theme().primary` and `primary_foreground` — the window's own primary-button fill on a label | **Stripped the chrome.** Tinted primary instead. |
| 5 | `awake-gui` records / rules / status | 7 `Tag` uses, two of them `Tag::primary()` (literal action fill) for "matching now" and "default preset" | **Stripped the chrome.** All are `StatusPill` now. |

### Intent calls made

- **查看修復方式 (`view_recovery`) — kept as an action, already wired.** The
  field report suspected it was a static red pill. It is not: `components.rs`
  builds it as `Button::danger().on_click(open_component)`, and
  `pages_settings.rs` builds the health-screen one as
  `Button.on_click(navigate(Page::Restore))`. Both were driven on the
  compositor and both navigated. What made it *read* as a decoration is that
  the filled red `可還原` **status** pill sat immediately to its left with the
  identical fill. Fixing the pill is what fixes the button.
- **重新啟動需求 — not a chip, no change.** In the component card it is plain
  muted text (`{label}: {value}`); on the detail and review screens it is a
  `key_value_row`. Neither has pill or button chrome. Confirmed on screen.
- **`AlreadyDefault` — status, not a disabled action.** Judged from the
  `PrimaryAction` doc comment, which already said "A component that is already
  the default gets a status, never a switch that would mean nothing." The
  implementation contradicted its own comment.

### Noted, not fixed

- **Sidebar footer (ticket 47's file — left alone by agreement).**
  `gpui_component::SidebarFooter` paints
  `.hover(bg(sidebar_accent), text_color(sidebar_accent_foreground))` on its
  whole row unconditionally (`crates/ui/src/sidebar/footer.rs:78`). In
  `manager-gui/src/shell.rs:108` that row is the static host profile
  ("zorin 24.04 / amd64") with no handler: it lights up like a menu item and
  does nothing. **The merge with ticket 47 should reconcile this.** The same
  applies to `monitor-gui/src/shell.rs:87`, `awake-gui/src/shell.rs:123` and
  `touchpad-gui/src/pages.rs:91` — one shared cause, four windows.
- **`c.restore_available` is one string doing three jobs.** It is the
  components-list pill, the failure card's ordinary recovery value, *and* the
  name of the `DoctorCheckKind::RestoreData` health check
  (`pages_settings.rs:181`), where "可還原上一個版本 · 通過" reads oddly as a
  check name. Out of scope here: a copy decision, not an affordance one.
- **`files-gui/src/shell.rs:449`** — a warning-filled note in the Devices
  sidebar section. Inspected and cleared: it stretches to the full column
  width, is not pill-shaped, and registers no hover. A banner, not a chip.

## The other applications

Audited at grep depth plus render reading, for: `Tag::` use, solid
`cx.theme().primary|danger|success|warning|info` backgrounds, and `Button`
instances with no `on_click`.

| Application | Result |
| --- | --- |
| `awake-gui` | 7 violations, all fixed (row 5 above) |
| `app-chooser-gui` | 1 violation, fixed (row 4 above) |
| `monitor-gui` | Clean. Its two action-color fills are a sidebar logo square and a sparkline bar. |
| `files-gui` | Clean. Its fills are a progress bar, a conflict banner and a sidebar note; the device row and the eject control are real buttons. |
| `touchpad-gui` | Clean. Its own `badge` (`pages.rs:304`) is outline-only with no fill and no hover — already compliant. Other fills are gesture dots and full-width banners. |
| `launcher-gui` | Clean. No `Tag`, no action-color fill. |
| `awake-tray` | Clean. No render code of this kind. |

No dead controls were found in any of the seven: a scan for `Button::new`
without a following `on_click`, `dropdown_menu` or `popup_menu` returned two
candidates, both false positives on inspection (`files-gui/src/shell.rs:668`
has its handler further down the chain; `pages_defaults.rs:311` was the
`AlreadyDefault` case, fixed as row 3).

## Tests

8 new, 2,579 passing across the workspace, 0 failing.

**`better-ui` (3)** — `a_status_pill_is_never_an_action`,
`every_tone_carries_its_label_unchanged`, and
`the_badge_primitive_registers_no_interactive_chrome`, which reads `badge`'s
own source and fails if it grows `.hover(`, `cursor_pointer`, `on_click` or
`.id(`.

**`manager-gui` (5, in `tests::affordance`)** —
`every_status_indicator_is_a_status_and_never_an_action` enumerates all 46
indicators the window can draw (12 component states × pending/not, 3 kinds, 3
health states, 3 doctor statuses, 7 activity kinds, 5 defaults aggregates) in
both locales; `a_failure_still_reads_as_a_failure` locks that the change did
not grey everything out; `a_pending_component_reads_as_pending_whatever_its_state`;
`a_component_already_the_default_leads_with_a_status_not_a_button`; and
`the_recovery_row_never_prints_its_own_label_as_its_value`.

**Three source-level regression tests** cover what a view model cannot prove
about a render function: `no_screen_draws_a_status_with_the_toolkit_tag`
(no `Tag::` in any of the five render files),
`the_failure_card_labels_its_recovery_row_as_a_heading`, and
`the_defaults_row_draws_no_button_for_a_component_already_the_default`.

Each was checked against the defect it describes: reverting the recovery-row
label makes `the_failure_card_labels_its_recovery_row_as_a_heading` fail.

## Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all --check` | pass |
| `cargo check --workspace` | pass |
| `cargo test --workspace` | 2,579 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| 8 s `ZED_HEADLESS=1` smoke | `manager-gui`, `awake-gui`, `app-chooser-gui` each survived the full 8 s |

## On-host verification

Run on the host's own GPUI stack in a nested sway sandbox at 1500×1000, dark
theme, zh-TW, `BETTER_MANAGER_OFFLINE=1`, scratch `XDG_STATE_HOME` seeded with
the same hand-built state for both builds: `better-monitor` failed at
`CheckingHealth` with a restore snapshot, `better-files` degraded and disabled.
The before build is `main` at `282f3c0`; the after build is this branch.

**Reproduced before the fix.** The components list drew `可還原` as a
*solid* red pill immediately left of the *solid* red `查看修復方式` button —
two identical-looking objects, one of them inert. The health screen drew
filled green `通過` and filled red `異常`. The restore screen printed
**可還原上一個版本 · 可還原上一個版本**, label and value the same string,
exactly as reported from the field.

**After.** Every row now has exactly one solid-filled element and it is the
button: `可還原`, `異常` and `可還原上一個版本` are tinted outlines, `查看修復
方式` is the only saturated red. `需要處理` on the Better Files row reads the
same way against its `啟用` button. The restore card's row reads
**復原狀態 · 可還原上一個版本**. `查看修復方式` was clicked on both the
components list and the health screen and navigated correctly in both.

**Not verified on screen.** The `AlreadyDefault` defaults row: this host has no
component that is currently the XDG default, so the Defaults screen only ever
produced the `Verify` arm. That fix rests on the test and on the code, not on a
photograph.
