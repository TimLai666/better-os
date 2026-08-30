# 32 — files-core and files-platform: typed locations, streaming listing, navigation

**Epic:** Better Files (Issue #6)
**User Story:** Opening a directory with a hundred thousand entries starts
showing files immediately instead of freezing, and a location is a typed thing
rather than a path string that happens to work today.
**Blocked by:** 18-app-catalog, 31-direct-removal-storage
**Status:** todo

## Goal

Lay Better Files' domain and platform foundation, with Issue #6's architectural
rule enforced from the first commit: do not treat every location as a local
`std::path::PathBuf`.

## What it delivers

- `files-core`: a typed location and URI abstraction able to represent local
  paths, Trash, Recent, Applications, and future SMB, SFTP, WebDAV, and
  GVfs- or portal-backed locations without any of them being a special-cased
  string.
- Progressive directory listing: results stream off the render thread, visible
  items are prioritized, metadata work is scheduled separately, and obsolete work
  is cancelled when the user navigates away.
- The navigation model: back and forward history, parent navigation, tabs, and
  the editable path or location field's state — as model, independent of any
  view.
- The entry model: name, type, size, timestamps, permissions, symlink status,
  and the metadata the list and grid views sort on.
- Hidden-entry rules taken from platform rules rather than filename inspection
  alone, with the preference represented in the model and revealing hidden
  entries not requiring a blocking full reload.
- `files-platform`: filesystem access, file watching with incremental refresh,
  and the integration points for XDG, MIME, and trash. Application resolution
  goes through the shared catalog from ticket 18; there is no second desktop-entry
  parser here.
- Caching bounded in memory with explicit invalidation, and no duplicate
  indexing.
- Edge-case coverage from the architecture stage: symlink loops, permission
  errors, disappearing devices, filename encoding, very long paths, case
  conflicts, and concurrent external changes.

## Out of scope

- The job engine and file operations (ticket 33).
- The GPUI window, views, and sidebar (ticket 34).
- Preview, search, the Applications location, and external-device integration
  (ticket 35).
- Split view, column view, restoring recently closed tabs, opening a terminal in
  the current location, and network backend implementations. The location model
  must not prevent them.
- Indexed full-disk search.

## Deferred decisions

Issue #6 requires an ADR or a focused follow-up issue rather than a silent
choice for: the Better Files license, whether GPLv3 COSMIC Files code will ever
be reused, per-folder versus global view preference defaults, network backend
implementation priority, and how much Elio code is reused after audit. No GPL,
AGPL, or FSL code is copied before the license decision is documented, and any
Elio reuse is audited, attributed, and isolated from TUI concerns first.

## Acceptance criteria

- [ ] Locations are a typed abstraction; no public API takes a bare
      `std::path::PathBuf` as the general location type.
- [ ] A directory listing streams progressively and never runs on the render
      thread.
- [ ] Navigating away cancels the obsolete listing and metadata work.
- [ ] The navigation model supports back, forward, parent, and tabs
      independently of any view.
- [ ] Hidden status comes from platform rules, and toggling it does not trigger a
      blocking full reload.
- [ ] File watching produces incremental refreshes rather than full re-listings.
- [ ] Application resolution uses the shared catalog from ticket 18, and
      `files-platform` contains no desktop-entry parser.
- [ ] Caches are bounded with explicit invalidation.
- [ ] Symlink loops, permission errors, disappearing devices, filename encoding,
      very long paths, case conflicts, and concurrent external changes each have
      a test.
- [ ] No GPL, AGPL, or FSL code is copied, and any reused Elio code is audited,
      attributed, and covered by Better Files tests.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench -p files-core`: time to first visible entries and full-listing
  time for a 100,000-entry synthetic directory, a mixed-media directory, and a
  deep tree
- A cancellation test proving a navigation away stops the previous listing's
  work
- A test asserting `files-core` and `files-platform` have no GPUI dependency
