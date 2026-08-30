# 18 — Shared application catalog: discovery, normalization, launching

**Epic:** Shared application catalog and Better App Chooser (Issue #4)
**User Story:** Better Files, Better Launcher, and every future Better OS surface
read one validated list of installed applications, and launch them through their
registered desktop definition instead of a guessed executable path.
**Blocked by:** none
**Status:** todo

## Goal

Create the one application-catalog layer the whole suite shares. Issue #4 states
the rule directly: do not implement separate desktop-entry scanners in Better
Files and Better Launcher. Everything downstream — the chooser (ticket 19), the
launcher index (ticket 20), the Applications location (ticket 35) — consumes
this crate pair and never re-parses a `.desktop` file itself.

## What it delivers

- `app-catalog-core`: the `ApplicationRecord` model and its normalization and
  validation rules. Fields at minimum: `desktop_id`, names and localized names,
  icon reference, categories, keywords, supported MIME types, source kind and
  source path, launch definition, visibility rules, desktop actions, executable
  resolution status, capability flags.
- Desktop entries are untrusted input. Parse and validate before any consumer
  sees a record, the same way `better-core` treats a component manifest.
- Visibility rules: `Hidden`, `NoDisplay`, `OnlyShowIn`/`NotShowIn`, and
  `TryExec` decide exclusion. `Terminal` is carried, not silently dropped.
- `app-catalog-platform`: XDG user and system application directory discovery,
  change watching through filesystem notification (no idle polling when
  notifications are available), and launching.
- Launching uses the registered desktop definition, passes selected files or
  URIs according to the entry's own field codes, validates launch targets, and
  never builds a shell string. `DBusActivatable` entries activate over D-Bus.
  Desktop actions are launchable by action id.
- Flatpak and Snap applications appear when they publish normal desktop
  entries. An AppImage appears only when it is already registered.
- Executable resolution is a reported status, not a fabricated path: a Flatpak,
  Snap, D-Bus-activated, or wrapper-based entry reports that no single canonical
  executable exists.
- Parsing and normalization run off the GPUI render thread; the crate carries no
  GPUI dependency at all.
- Benchmarks over a 5,000-record synthetic catalog: cold discovery, warm load,
  live refresh after an entry is added or removed, memory footprint.
- `docs/` note recording the application identity model and the integration
  boundary each consumer is allowed to cross.

## Out of scope

- The chooser, its ranking, and MIME association writes (ticket 19).
- The launcher index and ranking (ticket 20).
- The Applications virtual location and its rendering (tickets 32 and 35).
- Recursive disk scanning for arbitrary AppImage files.
- Editing or writing system desktop entries.
- A `applications://` URI scheme or any public URI form.
- Package uninstall or app-store behavior.

## Deferred decisions

Issue #4 requires an ADR or a focused follow-up issue rather than a silent
choice for: the exact category and grouping design, whether a public
`applications://` URI should exist, and how favorites and usage history are
shared with Better Launcher. Do not encode any of these in the record model.

## Acceptance criteria

- [ ] Applications are discovered from standard XDG user and system application
      directories.
- [ ] Hidden, `NoDisplay`, and desktop-incompatible entries are excluded, and a
      test proves each exclusion rule separately.
- [ ] A record carries desktop id, localized names, generic name, comment, icon,
      categories, keywords, MIME types, `Exec`, `TryExec`, D-Bus activation
      metadata, actions, terminal requirement, source path, and source kind.
- [ ] Launching goes through the registered desktop definition with no shell
      string concatenation anywhere in the path.
- [ ] Flatpak, Snap, D-Bus-activated, and wrapper-based entries never receive a
      fabricated executable path; their executable status says so.
- [ ] Malformed, truncated, and hostile desktop entries are rejected with stable
      machine keys rather than panicking or being partially accepted.
- [ ] Desktop-entry additions, changes, and removals are observed through
      filesystem notification, with no continuous polling while idle.
- [ ] Discovery and normalization contain no GPUI dependency and do not run on a
      render thread.
- [ ] Benchmarks over 5,000 synthetic records report cold discovery, warm load,
      refresh, and memory, with the tested hardware and dataset recorded.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench -p app-catalog-core` over the 5,000-record synthetic catalog,
  with results written into the ticket's benchmark note
- A launch smoke against a fixture desktop entry that records the argument
  vector it received, proving no shell interpretation happened
