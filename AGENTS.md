# Better OS Agent Contract

Better OS is a modular performance-improvement layer for Zorin OS and Ubuntu.
It is not a Linux distribution fork. First-party components are installed,
updated, verified, and rolled back through shared manager operations.

## Required artifacts

- `delivery-status.md` — read first on arrival and update after each milestone,
  blocker, or handoff.
- `ENG.md` — read before changing crate boundaries, public contracts, test
  seams, packaging, or privileged-operation design.
- `docs/tickets/` — select the next ticket whose blockers are complete.
- `docs/architecture.md` — read before changing the system decomposition.
- `docs/component-manifest.md` — read before changing manifest fields or
  validation rules.

## Working rules

- Keep privileged system mutation outside GUI and CLI code. It belongs in
  `manager-daemon`, reached over D-Bus and authorized by polkit. Read-only host
  queries such as `dpkg-query` are allowed unprivileged; changing the host is
  not.
- When testing an unreleased Better OS build or first-party component, use
  [Chefer](https://github.com/TimLai666/chefer) to package it as a disposable
  AppCipe and run it in an isolated containerized environment. Keep test data,
  mounts, and ports temporary or explicitly scoped. Never install the
  unreleased build directly on the host or touch host system paths, package
  state, or privileged services.
- Keep manager CLI and GUI on the same `manager-core` planning API.
- Use Rust for first-party production code. Use Go only after recording a
  concrete reason in an ADR. Do not add C, C++, Python, JavaScript, Electron,
  Tauri, or GTK application code.
- Use GPUI with `gpui-component` for first-party desktop GUIs. `better-ui` owns
  shared presentation primitives.
- Treat manifests as untrusted input: validate schema, targets, artifacts,
  dependencies, conflicts, and lifecycle metadata before planning.
- Do not add a public APT repository, signing implementation, or automatic
  optimizer without an explicit decision. The project license (ADR 0003) and
  the privileged daemon IPC protocol (ADR 0007) are decided; changing either
  needs a new ADR.
- Every behavior change needs tests. Run formatting, linting, workspace checks,
  and tests before handoff.

## Handoff

Before handoff, update `delivery-status.md`, keep the next ticket accurate,
record active blockers, and state which checks actually ran. Do not claim a
GUI or dependency compiles when the relevant command was not executed.

## Follow-ups

- Build each supported Ubuntu release in a compatible base environment. The
  current Zorin 18 host produces `libc6 (>= 2.39)` and must not supply a 22.04
  release artifact.
- Update GitHub Actions dependencies after the Node.js 20 deprecation warning
  on `actions/checkout` and `actions/upload-artifact` is addressed.
- Review the license implications of every copyleft dependency before release.
- Better Monitor now presents its collectors: `monitor-views` owns grouping and
  the table, apps, and overview models with no GPUI dependency, and
  `monitor-actions-linux` is the only crate that calls `kill(2)` or
  `setpriority(2)`. Process actions are own-user only; cross-user and elevated
  actions are refused with the reason shown, and the narrow polkit-reviewed
  boundary that would allow them is not built. The persistent history store and
  service are still outstanding.
- Decide the Better Monitor time-series storage engine and retention policy in
  an ADR before any collector output is persisted. Issue #16 lists SQLite as a
  candidate, not a decision.
- Decide whether `sysinfo` becomes a dependency for battery, component naming,
  or disk identity. It was evaluated and not adopted for the `/proc`-backed
  metrics; the reasoning is in `docs/monitor-collector-sources.md`.
- Decide the package signature format before offering a signed distribution
  channel. Checksums are currently the only integrity mechanism.
- Decide whether the daemon should offer a `dpkg --configure -a` repair action
  for a transaction interrupted by a crash or power loss.
- Align the declared Rust 1.85 baseline with the lockfile dependency MSRV
  before treating Rust 1.85 as a supported build target.
- Better Launcher is openable and usable but unmeasured. Its manifest defines
  warm search update, warm overlay open, application-list update, and idle
  overhead, and no harness runs any of them. Do not claim launcher performance
  until one does.
- `packaging/build-deb.sh` builds no `better-launcher` package, so that
  manifest's artifact checksums are placeholders and the component is not
  release-eligible. It is validated on every test run; it is not installable.
- Every Better OS desktop binary links an HTTP client. `gpui-component-assets`
  depends on `zed-reqwest`, which brings hyper and rustls, so "performs no
  network request" is provable for Better OS crates and not for the shipped
  binaries. `crates/launcher-gui/tests/dependencies.rs` states the exception
  with its one cause. Decide whether the toolkit dependency is acceptable, or
  whether the assets crate can be dropped, before a release claims otherwise.
- Better Launcher ships no gesture adapter and Better Defaults does not yet
  apply its keyboard shortcut, so the only activation paths that work out of
  the box are the desktop entry and running the binary. ADR 0008 and ticket 30
  own the first gap; ticket 27 owns the second.
- Let the GNOME defaults adapters adopt the dconf write path that now exists.
  Ticket 29 built it — `ca.desrt.dconf.Writer.Change` over the session bus, with
  the change set encoded in `touchpad-platform`'s `gvariant` module and pinned
  against GLib's own bytes — and ADR 0010 records the decision. Better Defaults
  still reports Manual action required for a change, because adopting the path
  changes its behaviour and its tests.
- Better Touchpad shows vertical scroll factor, horizontal scroll factor, and
  smooth scrolling as unavailable, because GNOME 46 has no key for any of them.
  The model and the mock backend carry all three; making them live is one table
  row in `touchpad-platform`'s GNOME backend when a GNOME that has the key is a
  supported target.
- Decide what restore should do when the previously selected application has
  been uninstalled. Restore writes the captured desktop entry either way and
  reports what the verifying read then saw, so the Defaults review screen has no
  "previous target no longer exists" class to show. Deciding needs the shared
  application catalog consulted at restore time.
- Decide whether `app-chooser-core` should offer a typed "remove this
  association" operation. Without one, restoring an XDG default that previously
  had no owner reports Manual action required rather than clearing the line, and
  no second `mimeapps.list` editor may be written.
- Capture a handler group's previous value per declared type. A group whose
  types currently point at different applications reads as unknown and is
  refused rather than flattened into one owner, which is safe but coarse.
- Decide which default integrations the shipped component manifests declare.
  Issue #10 defers it; the schema is proven against a fixture instead, so
  `better-manager defaults inspect` reports nothing against the built-in
  catalog and Better Manager's Defaults screen shows its empty state. Both
  surfaces work; there is simply nothing declared for them to show.
