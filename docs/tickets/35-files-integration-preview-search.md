# 35 — Better Files integration: Applications, devices, preview, search, benchmarks

**Epic:** Better Files (Issue #6)
**User Story:** Applications appear as a location in Better Files, an external
drive mounts when it is opened and disappears cleanly when it is unplugged, and
a file can be previewed and found without waiting on a full application.
**Blocked by:** 19-app-chooser, 34-files-gui
**Status:** todo

## Goal

Connect Better Files to the two components built for it — the shared application
catalog and the direct-removal storage layer — then add preview, current-directory
search, and the benchmark harness that makes the performance claim checkable.

## What it delivers

- The Applications virtual location as a first-class typed location with typed
  items and capabilities, backed by the shared catalog from ticket 18. Opening an
  item launches the application through its registered desktop definition rather
  than treating it as an executable file. Grid and list presentation, application
  icon and localized name, search and sorting, keyboard and pointer navigation,
  application details, source metadata for diagnostics, live refresh on
  desktop-entry changes, and a clear distinction between system-wide and per-user
  applications when requested.
- Context actions in scope: Open, Open New Window where a desktop action or
  supported activation path exists, View Details, Add to Favorites, and Use to
  Open Selected File when entered from chooser context.
- The location is not a real directory, a symlink farm, a copy of `.desktop`
  files, or a FUSE mount, and it never exposes an internal `.desktop` file as if
  it were the application.
- Open With in Better Files routed to Better App Chooser from ticket 19.
- External devices in the sidebar from ticket 31's typed state: detected devices
  appear before mounting; clicking mounts and opens automatically; leaving the
  location does not unmount; Ready to Unplug, Writing, Busy, Performance, and
  Unknown are visually distinct; Eject stays available; and the idle state stays
  visually quiet rather than a permanent warning console.
- Disconnect cleanup: unplugging an idle Direct Removal device removes it from
  the sidebar and clears stale mount and navigation state without user action. A
  device disconnected while being viewed returns Better Files to a safe location
  with an explanation. An unsafe removal during a write shows an accurate warning.
- File-operation completion from ticket 33 notifies the storage service so the
  filesystem-scoped flush happens after Better Files writes.
- `files-preview`: the preview interface plus image and plain-text implementations.
  Preview work runs off the render thread, is cancellable, enforces file-size and
  resource limits, treats untrusted file parsers as a security boundary, and
  degrades to metadata-only when safe rendering is unavailable.
- `files-search`: current-location search with the provider, ranking, and UI
  separated. Typing does not block navigation, results stream incrementally, and
  hidden files follow an explicit search setting.
- The benchmark harness covering Issue #6's scenarios, and the Better Manager
  component manifest for `better-files` plus its written rollback plan.

## Out of scope

- Complete indexed full-disk search and any indexing daemon.
- Network location backends and cloud-provider APIs.
- Package uninstall from the Applications location, editing desktop entries, and
  moving applications between system and user scope.
- Preview implementations beyond image and text — Markdown, PDF, media, archive,
  and folder summary — which the interface must not prevent.

## Deferred decisions

Issue #6 requires an ADR or a focused follow-up issue rather than a silent
choice for: the exact indexed-search engine, and the exact Windows comparison
hardware and benchmark datasets. Issue #4's deferred list also applies here:
whether the Applications view appears as a sidebar root, a Home child, or both,
and whether a public `applications://` URI exists, stay open. Any reused Elio
preview module needs its license, dependency, and security audit first.

## Acceptance criteria

- [ ] Better Files renders an Applications virtual location backed by registered
      desktop applications from the shared catalog.
- [ ] The Applications view is not a real directory, symlink farm, `.desktop`
      copy, or FUSE mount, and no internal `.desktop` file is presented as the
      application.
- [ ] Better Launcher and Better Files use the same application catalog layer.
- [ ] Selecting an application from the Applications view launches it through its
      registered desktop definition.
- [ ] Open With opens Better App Chooser and applies its selection.
- [ ] External devices mount automatically when opened, and leaving the location
      does not unmount them.
- [ ] An idle Direct Removal device can be unplugged and disappears cleanly from
      the sidebar without Eject first.
- [ ] Better Files never shows Ready to unplug while a write or flush is known
      pending.
- [ ] Disconnecting the currently viewed device returns Better Files to a safe
      location with an explanation, and leaves no stale navigation state.
- [ ] An unsafe removal during a write produces an explicit warning rather than a
      clean-completion message.
- [ ] Preview work and directory scanning never run on the GPUI render thread,
      and preview is cancellable and size-limited.
- [ ] Current-directory search streams results and does not block navigation.
- [ ] The benchmark harness runs the named scenarios and any public performance
      claim states the workflow, dataset, hardware, and metric.
- [ ] A valid Better OS manifest and a written rollback plan exist for the
      component.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `files-gui` reaching the Applications location
- The benchmark harness over Issue #6's scenarios: 100,000-entry directory
  progressive render, mixed media with thumbnails, deep tree, large sequential
  copy, 100,000 small-file copy, same- and cross-filesystem move,
  current-directory search p50/p95, multi-tab navigation, startup with restored
  tabs and unavailable locations, and external-device connect, open, and
  disconnect — compared against Nautilus, and against Windows File Explorer for
  the named scenarios where practical
- A device-disconnect smoke driven through ticket 31's service, asserting sidebar
  and navigation cleanup
- Manifest validation through `better-core` against the new manifest
