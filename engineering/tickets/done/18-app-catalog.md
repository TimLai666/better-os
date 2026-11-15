# 18 — Shared application catalog: discovery, normalization, launching

**Epic:** Shared application catalog and Better App Chooser (Issue #4)
**User Story:** Better Files, Better Launcher, and every future Better OS surface
read one validated list of installed applications, and launch them through their
registered desktop definition instead of a guessed executable path.
**Blocked by:** none

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

- [x] Applications are discovered from standard XDG user and system application
      directories.
- [x] Hidden, `NoDisplay`, and desktop-incompatible entries are excluded, and a
      test proves each exclusion rule separately.
- [x] A record carries desktop id, localized names, generic name, comment, icon,
      categories, keywords, MIME types, `Exec`, `TryExec`, D-Bus activation
      metadata, actions, terminal requirement, source path, and source kind.
- [x] Launching goes through the registered desktop definition with no shell
      string concatenation anywhere in the path.
- [x] Flatpak, Snap, D-Bus-activated, and wrapper-based entries never receive a
      fabricated executable path; their executable status says so.
- [x] Malformed, truncated, and hostile desktop entries are rejected with stable
      machine keys rather than panicking or being partially accepted.
- [x] Desktop-entry additions, changes, and removals are observed through
      filesystem notification, with no continuous polling while idle.
- [x] Discovery and normalization contain no GPUI dependency and do not run on a
      render thread.
- [x] Benchmarks over 5,000 synthetic records report cold discovery, warm load,
      refresh, and memory, with the tested hardware and dataset recorded.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench -p app-catalog-core` over the 5,000-record synthetic catalog,
  with results written into the benchmark note in
  `engineering/specs/app-catalog-identity.md`
- A launch smoke against a fixture desktop entry that records the argument
  vector it received, proving no shell interpretation happened

## Notes

Carried over from the retired delivery status record.

Tickets 18 through 35 were cut from the eight open component issues. The cut is
by deliverable, not by crate layer: each ticket is something a user or a
consuming component can actually use, and the crate boundaries follow from that
rather than the other way around. Every first-implementation-scope item in each
issue is placed in exactly one ticket, and each ticket carries a deferred-
decisions note naming the ADRs its issue demands instead of a silent choice.

Two tickets are shared infrastructure that everything else waits on: ticket 18
is the single desktop-entry scanner for the whole suite, and ticket 31 is the
external-storage layer Better Files reads rather than reimplements. Neither has
a blocker, and both should land before their consumers start.

No implementation code was written in this round. The workspace still contains
the eleven Manager and Monitor crates; every crate named in tickets 18 through
35 is planned, not present.

Ticket 18 landed the shared application catalog: `app-catalog-core` (the record
model, desktop-entry parsing, `Exec` tokenization and field codes, precedence
and visibility rules) and `app-catalog-platform` (XDG directory discovery,
inotify-based change watching, launching). Both are in the workspace and
neither depends on GPUI.

The two invariants the whole suite now inherits: an application's identity is
its desktop ID and never a path, and a launch produces argument vectors or a
D-Bus activation with no code path that can build a command string. Executable
resolution is a reported status, so a Flatpak, Snap, AppImage, wrapper, or
D-Bus-activated entry says it has no canonical executable instead of offering
one. `engineering/specs/app-catalog-identity.md` records the model, the boundary each
consumer may cross, and the benchmark results.

99 tests were added: 68 core unit tests, 10 core fixture-tree tests, 12
platform unit tests, 8 discovery and watching tests against real directories,
and 2 launch smoke tests that read back the `argv` a real spawned process
received. Local `cargo fmt --all -- --check`, `cargo check --workspace`,
`cargo test --workspace` (23 suites, no failures), `cargo clippy --workspace
--all-targets -- -D warnings`, the same clippy run with the `dbus-activation`
feature, and `cargo bench -p app-catalog-core` all passed.

Benchmarks over 5,000 synthetic records on an AMD Ryzen AI 9 HX 370: 44.6 ms
cold discovery, 40.4 ms warm load, 40.5 ms refresh after an entry is added,
20.1 MB resident. Resolving executables against the real `PATH` adds 113 ms,
which is the largest single cost in a full load and is worth knowing before a
consumer asks for it.

D-Bus activation ships behind the off-by-default `dbus-activation` feature, the
same shape as `manager-platform`'s `dbus-client`. It compiles and is covered by
a recording activator, but it has not been exercised against a real activatable
application on a session bus.
