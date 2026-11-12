# Ticket 84 — Control center: the shell, Home, and Settings

**Blocked by:** 80, 83 · **Status:** ready

## Why

ADR 0017 makes Better Manager the one control center. Every page ticket
(85–90) needs the same window around it first: the navigation, the search,
the back button, deep links, one language and appearance setting, and the
Home page that collects what needs attention. Building that once, with a
place for each page to plug in, is what lets 85–90 run in parallel without
editing the same files.

## What to build

The layout, states, and copy are in `design/control-center-spec.md` sections
1–3 and 10. Use `better_ui::fluent` controls only.

- **Window.** `crates/manager-gui` on `fluent::chrome`, `NavigationView`
  (320px, compact 48px below 1000px with ☰ and tooltips), the eight items in
  the spec's order with their icons, titlebar back button with page history
  and Alt+Left, content column at most 1000px.
- **Page registry.** A `pages/` directory with one module per page and one
  registration list. Each page module exposes, without GPUI: its route name,
  its title, its search entries (card header, description, group), and
  `attention(&Snapshot) -> Vec<AttentionItem>`. A page ticket adds its module
  and one line to the list and touches nothing else in the shell. Pages that
  later tickets build are registered here as placeholders that show the
  "not built yet" state only in debug builds; a release build must not ship
  until 85–90 have replaced them (ticket 91 checks this).
- **Search.** "Find a setting" in the pane: prefix-of-word match over the
  current language's entries, at most eight suggestions, Enter opens the page
  and focuses the card. Matching lives in a GPUI-free module with tests (both
  languages, empty query, no result, CJK text with no word breaks: match any
  substring for CJK).
- **Deep links and one instance.** `better-manager --page <page>` with the
  nine page names and `keep-awake/until` (spec section 1); the window owns `io.betteros.Manager` on the session bus
  and serves `ShowPage(s)`; a second invocation calls it and exits 0. Unknown
  page refused in both languages. Update `better-manager --help` and the
  invocation tests in `main.rs`.
- **Home.** Device banner from DMI and the battery, attention cards from every
  registered page's `attention`, the first-run info bar, quick settings, and
  recent activity, as the spec says. The Apps attention items (dpkg
  interrupted, failed or degraded component, drift, updates, catalog) are
  written here from today's `manager-core` state, since they exist now.
- **Settings.** Appearance (`theme::Appearance`, dark/light/system followed
  live, stored once), Language (system/English/繁體中文, applied without
  restart), Check for updates when opened (reads today's `check_updates`;
  ticket 85 makes the launch refresh obey it), Update channel read-only,
  About with version, source link, and third-party licenses link, the privacy
  line. Drop `auto_download` and `diagnostic_logs` from the UI; keep the
  fields in the store so an older manager reading the file still parses it.
- **Kit additions, in `crates/better-ui/src/fluent/` with tests and a
  gallery page each:** a search box slot with suggestions on
  `NavigationView` (and its compact behaviour), an info badge on `NavItem`
  (count and dot), and the titlebar back button.
- **Shared states.** One GPUI-free module for the states table in spec
  section 2 (not installed, too old, service not running, unreadable, no
  permission, not allowed, write failed, read back different), with their
  copy, used by every page ticket.

## Acceptance

- [ ] Every element in spec sections 1–3 and 10 is on screen with the exact
      copy, in zh-TW and English, light and dark, at 1600 and 960 wide,
      screenshotted and looked at. Nothing truncates at 960 in either
      language.
- [ ] The pane collapses to 48px at 960 with labelled tooltips and the search
      still reachable (audit M5).
- [ ] `better-manager --page system` opens on System; with a window already
      open it switches that window and no second window appears (shown in the
      sandbox with `dbus-run-session`).
- [ ] Search finds "充電" and "charge" to the charge-limit card once ticket 86
      registers it; until then a test with fixture entries covers matching.
- [ ] Home shows "Everything's fine" with no warning items and lists the
      Apps items from a fixture state with each kind of fault.
- [ ] Unit tests for search, the page registry, attention ordering, the
      deep-link argument rules, and the kit additions. No new external
      package in `Cargo.lock`; if it changes, `docs/third-party-licenses.md`
      is regenerated.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## Notes

Running `manager-gui` in the sandbox connects to the real system daemon: look,
don't press anything that changes packages. HOME and the XDG directories
point at a scratch directory.
