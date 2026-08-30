# 32 — files-core and files-platform: typed locations, streaming listing, navigation

**Epic:** Better Files (Issue #6)
**User Story:** Opening a directory with a hundred thousand entries starts
showing files immediately instead of freezing, and a location is a typed thing
rather than a path string that happens to work today.
**Blocked by:** 18-app-catalog, 31-direct-removal-storage
**Status:** implemented on branch `ticket-32`, not merged

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

- [x] Locations are a typed abstraction; no public API takes a bare
      `std::path::PathBuf` as the general location type. `Location` is a closed
      enum, `Location::as_local_path` is the only way to a path and answers
      `None` for Applications, Recent, Trash, network, and unsupported
      locations. A device location needs a mount point supplied before it
      resolves at all.
- [x] A directory listing streams progressively and never runs on the render
      thread. `LocalDirectoryReader` spawns a reader thread that pushes into a
      `ListingSink`; `Pane::pump` drains without blocking.
- [x] Navigating away cancels the obsolete listing and metadata work.
      Cancellation is checked before each entry's `stat`, so the reader stops
      after at most one more entry.
- [x] The navigation model supports back, forward, parent, and tabs
      independently of any view. `TabSet` also restores recently closed tabs
      with their history intact.
- [x] Hidden status comes from platform rules, and toggling it does not trigger a
      blocking full reload. Dotfiles and per-directory `.hidden` files, with the
      reason carried on each entry; the preference is a filter over indices.
- [x] File watching produces incremental refreshes rather than full re-listings.
      `refresh_for` `stat`s only the path that changed and produces a single
      row's add, modify, or remove. A dropped-event queue reports
      `Resynchronize` instead of leaving a wrong list.
- [x] Application resolution uses the shared catalog from ticket 18, and
      `files-platform` contains no desktop-entry parser. Asserted by a test that
      greps the platform sources.
- [x] Caches are bounded with explicit invalidation. `ListingCache` bounds by
      total entry count, not by number of locations, and has no time-based
      expiry.
- [x] Symlink loops, permission errors, disappearing devices, filename encoding,
      very long paths, case conflicts, and concurrent external changes each have
      a test. Two of them are narrower than the wording suggests and are named
      under "What is not covered" below.
- [x] No GPL, AGPL, or FSL code is copied, and any reused Elio code is audited,
      attributed, and covered by Better Files tests. Nothing was copied from any
      source; every line here is new. The licence decision the ticket defers is
      untouched.

## Verification

Run on branch `ticket-32`, crate-scoped, because a workspace-wide run rebuilds
the GPUI world in this worktree. The full workspace gate belongs on the main
checkout after merge and has **not** been run here.

- `cargo fmt --all -- --check` — passed, whole workspace
- `cargo check -p files-core -p files-platform` — passed
- `cargo test -p files-core -p files-platform` — passed, 145 tests
- `cargo clippy -p files-core -p files-platform --all-targets -- -D warnings` —
  passed
- `cargo bench -p files-core` — ran; numbers in
  `docs/files-listing-performance.md`
- Cancellation is proved three ways: a `files-core` unit test whose fake reader
  counts entries it actually produced and produces none after cancellation, a
  `files-platform` test against a real 2,000-entry directory, and the
  benchmark's cancellation latency, which stopped a 100,000-entry listing after
  256 entries in 0.021 ms
- `crates/files-core/tests/no_gpui_dependency.rs` walks the real dependency
  closure in `Cargo.lock` and fails if GPUI appears anywhere in it

### Headline numbers

100,000-entry flat directory: first batch of 256 entries visible in **1.6 ms**,
full listing in **125 ms**, model memory **38.3 MB**, cancellation latency
**0.021 ms**. Full table and the two performance bugs these numbers exposed are
in `docs/files-listing-performance.md`.

### What is not covered

- **A real over-`PATH_MAX` path is not created.** Building one requires walking
  into it with relative components, which changes the process working directory
  and is not safe from a test. What is tested is a 3,000-character path listing
  correctly, a 255-byte filename, and that the kernel's `ENAMETOOLONG` is
  translated rather than folded into a generic error.
- **A device is not really unplugged.** The test covers the translation the
  listing depends on: `ENODEV` and `ENXIO` become `ListingError::DeviceLost`,
  distinct from a permission error and from a generic failure. Ticket 31 records
  the same limitation for the storage crates.
- **The case-conflict test adapts to the host.** On a case-insensitive
  filesystem the two files collapse into one, and the test asserts what actually
  happened rather than assuming a case-sensitive host.
- **A frame can cost about 28 ms while a 100,000-entry directory loads.** The
  merge is proportional to the whole list. Precomputing a sort key per entry is
  the next step and is a follow-up, not something this ticket closed.
- **All benchmark numbers are warm-cache.** Cold-cache listing over a spinning
  disk or a USB stick is not measured.
- **`Location::Recent` is modelled but not listed.** Nothing populates it; it
  reports as not listable. The same is true of every network scheme, which is
  what the ticket's scope says.
- **`storage-service` is not consulted.** `MountTable` builds the weakest honest
  identity a mount table supports — volatile, never persistable — and says so.
  A consumer that has a real device identity from `storage-service` should
  prefer it.
