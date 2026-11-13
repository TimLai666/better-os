# Ticket 84 — Control center: the shell, Home, and Settings

**Blocked by:** 80, 83 · **Status:** released in v0.2.12

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

## What was built

- **Window.** `manager-gui` now draws `fluent::chrome::TitleBar` (title
  `Better Manager` in both languages, a back button while there is history)
  and a `NavigationView` with the eight items, their icons, and Settings in
  the footer. The pane is 320px, and below 1000px logical width it is 48px
  with labelled tooltips; opening it there floats it over the content, and a
  press outside closes it. Content starts 24px from the pane and is at most
  1000px wide. Alt+Left and the mouse back button go back. History skips the
  steps of a running flow (review, installing, results). The minimum window
  is 960 × 600.
- **Page registry.** `manager-gui/src/pages.rs` holds `PAGES`, one entry per
  page with its `--page` name, status, title, search entries, `attention`,
  and nav badge, all without GPUI. Home and Settings are `Built`; System,
  Touchpad, Keep awake, and Fixes are `Placeholder` (a debug build shows
  "This page isn't built yet", naming tickets 86, 88, 87, 90); Apps and Default
  apps are `Interim`: today's Better Manager and Defaults screens, unchanged,
  inside the new content column until tickets 85 and 89. Ticket 91's release
  gate can refuse anything that is not `Built`. The allowed file list for this
  ticket had no `pages/` directory, so Home and Settings live in `home.rs` and
  `settings_page.rs`; a page ticket adds its module (for example
  `pages/system/`) and replaces its one line in `PAGES`.
- **Shared states.** `pages::states` holds spec section 2's table: which kit
  control draws each state, its copy in both languages, the 300 ms loading
  rule, and when "Start service" is offered. Nothing outside the tests reads
  it yet, because no built page has a component behind it.
- **Search.** `search.rs`: prefix-of-word, case-insensitive for Latin text,
  substring for CJK, header matches before description matches, at most
  eight. The pane's `AutoSuggestBox` shows `{header}` over `{page} › {group}`,
  "No settings match…" when nothing does; Enter or a click opens the page,
  scrolls the card into view, and puts keyboard focus on it. Only Settings
  registers entries now; the "充電"/"charge" case is covered by a test with
  fixture entries until ticket 86 registers the charge-limit card.
- **Deep links and one instance.** `deep_link.rs`: `--page` with the nine
  names and `keep-awake/until` (which sets a flag for ticket 87's dialog),
  `--component` kept for Better Launcher. An unknown page is refused in both
  languages with status 2 before any window or bus. The window owns
  `io.betteros.Manager` and serves `ShowPage(s)` at `/io/betteros/Manager`; a
  second invocation calls it and exits 0. The bus accepts nothing the command
  line would not. `--help` lists the option and every page.
- **Home.** Device banner on the spec's two lines, from DMI (`product_name`
  with the repeated model suffix removed), `os-release`, and the system
  battery (a peripheral's battery is skipped). The attention group gathers every page's items in
  severity then table order, with "Everything's fine" only when nothing is a
  warning or error. The Apps items (dpkg interrupted, failed or degraded
  component, drift, updates, catalog) come from today's `manager-core` state.
  The first-run info bar sets `onboarding_complete` when closed. Quick
  settings has the Appearance card; Power mode and Keep awake are left to
  tickets 86 and 87. Recent activity lists the five newest entries with a
  status glyph and how long ago; `ActivityRecord` gained an optional
  `recorded_at_unix_seconds` (older files load without it, older managers
  ignore it).
- **Settings.** Appearance through `theme::Appearance` (dark, light, system
  followed live), language applied without restart (the pane's search box is
  rebuilt in the new language), Check for updates when opened (today's
  `check_updates`), Update channel read only, and an About expander with the
  version, source link, third-party licenses link (the installed
  `THIRD-PARTY-LICENSES.md`, or the repository's copy), and the privacy line.
  `auto_download`, `diagnostic_logs`, and the Preview channel are no longer
  offered and stay in the stored settings. The old first-run, Overview, and
  Settings screens and their copy are removed.
- **Kit additions** (`crates/better-ui/src/fluent/`, with tests and gallery
  rows): `auto_suggest` (`AutoSuggestBox` and its key rules), `badge`
  (`InfoBadge` count and dot), `NavigationView` search slot (field when open,
  search button when compact) and overlay mode and per-item badges,
  `TitleBar::back` with `is_back_shortcut`, `hyperlink` (`HyperlinkButton`),
  `SettingsCard::status`, `ListRow::status_glyph`, and `DeviceBanner`. The
  badge sizes follow the mockups, because WinUI's badge resources were not in
  the XAML copy the kit was read from.

### Decisions to look at

- Home reads dpkg's record itself, unprivileged, by the rule the daemon
  applies before a repair (`manager_core::dpkg_record`), so the
  interrupted-package card shows without starting the privileged service.
- English "{n} updates available" reads "1 update available" for one.
- `zbus` became a direct dependency of `manager-gui` (already in the lock
  through `manager-platform`); `Cargo.lock` gained that one edge and
  `docs/third-party-licenses.md` was regenerated.

### Not done or not verified

- The quick Power mode and Keep awake cards, the nav dot, and every page's
  own items wait for tickets 86–90.
- The dpkg-interrupted card was shown by tests only; this machine's dpkg is
  at rest and nothing here may interrupt it.
- "Use system setting" for Appearance was not switched against a desktop
  that changes its own appearance; headless sway has none.
- Not run on GNOME/Mutter, with a screen reader, or with touch.
- The two links were not clicked in the sandbox (they would open a browser).

### Screenshots

Headless sway (wayhand sandbox), `HOME` and the XDG directories in a scratch
directory, `BETTER_OS_TEST_SYSTEMCTL_GUARD` set so no `systemctl --user` ran,
`dbus-run-session` for every window; nothing that changes packages was
pressed. In
`/tmp/claude-1000/-home-tim-Developer-better-os/3b559518-c3b2-4104-bc77-d1c2e845c29a/scratchpad/shots-84/`:
`01-home-zh-dark-1600`, `02-search-zh-suggestion`,
`03-settings-zh-dark-1600-search-focused-language`,
`04-settings-en-dark-1600-language-live`, `05-home-en-dark-1600-after-alt-left`,
`06-home-en-light-1600`, `07-system-placeholder-en-light-1600`,
`08-apps-interim-en-light-1600`, `09-home-en-light-960-compact`,
`10-compact-tooltip-en-light-960`, `11-compact-search-overlay-en-light-960`,
`12-search-no-result-en-light-960`, `13-settings-en-light-960`,
`14-settings-zh-light-960`, `15-home-zh-light-960`, `16-home-zh-dark-960`,
`17-settings-zh-dark-960`, `18-settings-en-dark-960`,
`19-home-en-dark-960-after-back-click`,
`20-home-zh-dark-1600-fixture-faults-demo`,
`21-attention-view-opens-component-details-zh-dark-1600`,
`22-home-en-dark-1600-everything-fine-demo`, `23-settings-en-light-1600`,
`24-settings-zh-light-1600`, `25-home-zh-light-1600-everything-fine`,
`26-deeplink-second-invocation-switched-to-system-one-window`,
`27-deeplink-fresh-start-page-system`,
`28-gallery-navigation-search-badges-dark`, `29-gallery-titlebar-back-dark`,
`30-gallery-hyperlink-dark` (all `.png`).

### Independent review, and what it changed

An independent review of the first version found these, and each was fixed
after merging `main` (0.2.11, tickets 81 and 82):

1. Updates, Health (the doctor, restore), Activity, the manual update check,
   and Update all were unreachable: the pane no longer listed them and the
   interim Apps page linked nowhere. Apps now opens with the spec's three
   sub-page cards (Updates with its count, Health, Activity) above today's
   component list, and the search finds all three (`pages::apps_links`,
   test `apps_opens_updates_health_and_activity`).
2. Back, Alt+Left, the mouse back button, and pane clicks could leave a
   running transaction with no way back. While one runs, Back from anywhere
   returns to its progress screen, the progress screen has no Back, and the
   Apps item lands on it (`navigation::back_target`, `shows_back`,
   `landing`, test `a_running_transaction_is_where_back_and_apps_lead`).
3. The second instance could not pass its Wayland activation token. GPUI
   uses only the startup token a process is born with, when it creates a
   window, and gives an open window no way to use one handed to it later, so
   the token is not passed and the running window's `activate_window` may be
   refused by the compositor (GPUI's own comment says so). The desktop entry
   now sets `StartupNotify=false`, so a second launch, which exits without a
   window, leaves no busy pointer (test
   `the_desktop_entry_asks_for_no_startup_notification`). Raising the
   running window is therefore not guaranteed on GNOME; that is a GPUI
   limit. Not verified on GNOME: whether the first window still gets focus
   without a startup token.
4. The interrupted-dpkg card needed the daemon to be running. It now comes
   from an unprivileged read of `/var/lib/dpkg` (numbered journal entries in
   `updates/`, unfinished package states in `status`), at launch and
   whenever Home is shown (`manager_core::dpkg_record`, four tests). The
   rule is a copy of `manager-daemon`'s `dpkg_state`, which this ticket could
   not move into a shared crate.
5. The check of whether the daemon was running could race with it exiting
   and re-activate it. That check is gone with item 4: nothing at launch
   talks to the system bus.
6. Handing the page to a running window had no timeout. The connection now
   gives up after 5 seconds; tests cover a name held by something that is
   not Better Manager and a holder that never answers.
7. The private-bus tests kill their `dbus-daemon` in a `Drop` guard.
8. The banner is back to the spec's two lines with its own glyph
   (`PanelBottom`, not the touchpad's `Frame`; `DeviceBanner::detail` was
   removed). The gallery shows `DeviceBanner`, `SettingsCard::status`, and
   `ListRow::status_glyph`. Today's screens inside Apps and Default apps had
   been drawn in their narrow layout at every width; they are narrow only in
   a narrow window now (`navigation::legacy_compact`).

Screenshots of the changed states, same directory:
`31-review-home-two-line-banner-zh-dark-1920`,
`32-review-apps-subpage-cards-not-compact-zh-dark-1920`,
`33-review-home-during-demo-transaction`,
`34-review-back-returns-to-installing-no-back-button`,
`35-review-gallery-status-card-device-banner-zh-dark`,
`36-review-gallery-status-glyph-rows-zh-dark`. The transaction in 33 and 34
is demo mode's simulation and was cancelled afterwards; nothing on the
machine changed. Each window ran in its own `dbus-run-session` whose script
ends every process started on that bus when the window closes.

### Checks

`cargo fmt --all -- --check`, `cargo +1.99.0 clippy` and stable `cargo clippy
--workspace --all-targets --all-features --locked -- -D warnings`, `cargo +1.95
check --workspace --all-targets --locked`, and
`packaging/generate-third-party-notices.sh --check` pass. `cargo test
--workspace --locked --no-fail-fast` after the review fixes: 3430 passed, 19 failed, all 19 in
`app-catalog-platform`, `awake-platform`, `files-platform`, and
`launcher-platform` with `inotify_init` returning "Too many open files": the
host had 127 of its 128 inotify instances in use, held by other sessions'
processes. This ticket does not touch those crates. In the first round they
passed on their own once instances were freed; in the second the host had
none to spare even for a serial run.
