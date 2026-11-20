# Ticket 231 — The control center's categories, link cards, and new object icons

**Spec:** `design/system-settings-spec.md` sections 4, 5, 2.3, 16; ADR 0042 (proposed)
**Blocked by:** the owner's decision M3, and the owner's decision M1

## Why

Every system setting has to be findable in one place before any page is
built, or the person keeps two settings programs in their head. This ticket
gives Better Manager the pane of section 4.1, the category landings, and a
link card for every area whose page is not built yet, so later tickets only
replace a link card with a page.

## What to build

- The pane of section 4.1 (`navigation.rs`'s `Route`, `shell.rs`'s icons,
  `pages.rs`'s titles), the System landing of section 4.2, and the other
  category landings with their sub-page and link cards; today's System cards
  moved as section 4.2 says, each keeping its copy and behaviour; Touchpad
  under Bluetooth and devices, Window attention as a group of Notifications
  (a link card to today's page until ticket 238), Default apps inside Apps.
- Link cards (section 5.2): one module with no GPUI dependency that maps an
  area to COSMIC Settings' page name and GNOME Settings' panel (section 2.3)
  by `better_core::desktop::CurrentDesktop`, says when neither has it, and
  launches the program through its desktop entry, never a shell.
- Deep links of section 4.3; today's names keep working.
- Search entries for every landing and link card, with section 4.4's
  synonyms.
- The pane's items scroll above a fixed footer (`better_ui::fluent::navigation`).
- The object icons of section 16 in `better-ui`'s icon set, to DESIGN.md's
  Icons rules, shown on the gallery's Icons page.
- If the owner chose M3 (b), only the link cards, search, and icons, under
  today's pane.

## Done when

- [ ] Tests: every `Route` has a title in both languages and an icon; every
      link card names its program per desktop, including "neither" and "not
      installed"; every old `--page` name opens the page it moved to.
- [ ] Search finds 印表機, Wi-Fi, 時區, and 夜燈 in both languages and opens the
      right card.
- [ ] Rendered in a headless sway session (AGENTS.md's sandbox rule) in both
      appearances at 960 × 600 and 1400 × 900; screenshots in
      `/home/tim/Developer/better-os-evidence/231/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
