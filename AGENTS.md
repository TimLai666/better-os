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
  Tauri, or GTK application code. **One bounded exception exists**, granted by
  the project owner on 2026-08-31 and recorded in ADR 0012: GJS is permitted
  inside the Better Touchpad GNOME Shell adapter extension
  (`adapters/gnome-shell-touchpad/`) and nowhere else. Inside it, JavaScript may
  only bridge typed events and typed actions — reporting what the compositor
  saw, and invoking something GNOME Shell already exposes. It may hold no
  threshold, no cooldown, no cancellation rule, no conflict decision, no
  configuration, and no gesture decision, it may execute nothing, and no method
  it serves may take free text. `crates/touchpad-session/tests/extension.rs`
  asserts those bounds against the file. Widening them needs a new ADR, not a
  commit.
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
  boundary that would allow them is not built.
- `monitor-service` owns collection. `monitor-gui` has no sampler: it reaches
  the service over `org.betteros.Monitor1`, and where there is no service it
  runs the same engine in-process and says so. Do not add a collector call to
  GUI or CLI code.
- ADR 0011 records the Better Monitor history store as an **interim** append-log
  engine with measured numbers, a six-hour retention window, and a 64 MiB
  budget. The final engine decision is still owed and needs SQLite benchmarked
  in a network-capable environment; crates.io is unreachable from this one.
- Better Monitor's export is the only path data leaves the machine, and it has
  no network code. Redaction drops command-line arguments whole and replaces
  known personal values, addresses, identifiers, and credential-shaped text. A
  seeded-secret test asserts the boundary; keep it passing.
- Decide whether `sysinfo` becomes a dependency for battery, component naming,
  or disk identity. It was evaluated and not adopted for the `/proc`-backed
  metrics; the reasoning is in `docs/monitor-collector-sources.md`.
- Decide the package signature format before offering a signed distribution
  channel. Checksums are currently the only integrity mechanism.
- Decide whether the daemon should offer a `dpkg --configure -a` repair action
  for a transaction interrupted by a crash or power loss.
- Align the declared Rust 1.85 baseline with the lockfile dependency MSRV
  before treating Rust 1.85 as a supported build target.
- Better Launcher is measured now: `cargo bench -p launcher-gui --bench
  launcher_suite` runs all five manifest benchmarks and
  `docs/launcher-performance.md` records the numbers and the hardware. Two
  limits travel with them. `warm-overlay-open` ends at the first renderable
  model, because `ZED_HEADLESS=1` has no compositor and therefore no frame — no
  Better OS component has a time-to-photon figure. And the idle CPU figure is a
  headless one: nothing asks the window to repaint, so it is the launcher's own
  idle cost and not a claim about a launcher on a running desktop.
- All eight packages are published. `v0.2.1` carries every component
  `packaging/build-deb.sh` builds, for Ubuntu 22.04 and 24.04 on amd64 and
  arm64, and every shipped manifest now records the checksum of its own
  published 0.2.1 asset, verified after re-downloading the public release. No
  first party manifest carries a placeholder checksum any more;
  `components/manifests/better-files-example.yaml` is the one exception and is a
  schema fixture, not a released component. A version bump puts the placeholders
  back for the length of the release branch, because the checksums a manifest
  carries describe the previous release's packages and not the ones about to be
  built — write the real values back only after the new release is public and
  its assets have been downloaded again, and correct the comment above each
  block at the same time rather than leaving it describing the release before.
- A published binary embeds the manifests as they stood when it was built, and a
  manifest can only record a real checksum after its own release is public. So
  the `better-manager` shipped in a release always carries the pre-release
  placeholder checksums for that release, and only the *next* release ships a
  built-in catalog that can verify the previous one. Decide whether the catalog
  should be fetched rather than compiled in before treating the built-in catalog
  as an install path for the release it belongs to.
- `better-monitor` ships the window, the session service, the command line, and
  a systemd user unit in one package. v0.1.0 published the window alone, so the
  wider package carries its own version: it moved to 0.2.0 and travels with the
  workspace since. One version number no longer names two payloads.
- The command line's own `--help` says `better-monitor`, and the window already
  owns `/usr/bin/better-monitor` in a published package, so the CLI is installed
  as `better-monitor-cli`. Decide which of the two is renamed; a packaging
  change cannot fix a name a crate hard-codes.
- `better-touchpad` now has a manifest. Its health checks are the IDs
  `touchpad-core` emits and its benchmark baselines are the figures in
  `docs/touchpad-sensitivity-mapping.md`, but nothing runs those benchmarks —
  the same unenforced-budget gap `better-files.yaml` carries. Its checksums are
  the published v0.2.1 ones.
- A package installs its systemd user unit and does not enable it, matching
  `better-manager-daemon`. Nothing in dpkg stops a running Better Awake, Better
  Monitor, or Better Storage user service at removal either; the manifests'
  remove and rollback plans are where that belongs, and `better-awake.yaml`'s
  release notes describe behaviour the manager owns rather than the package.
- Better Awake detects nine of Issue #13's eleven trigger kinds. Fullscreen
  needs a compositor adapter and reports itself unavailable; audio reads ALSA and
  cannot see Bluetooth or network sinks. Both limits are recorded in ADR 0010 and
  in the provider modules. Do not claim full trigger coverage.
- A Better Awake low-battery stop writes a history entry and prints to stderr.
  The desktop notification belongs to the tray and is not wired, so a stop is
  visible in the log and in History but does not raise a notification.
- Every Better OS desktop binary links an HTTP client. `gpui-component-assets`
  depends on `zed-reqwest`, which brings hyper and rustls, so "performs no
  network request" is provable for Better OS crates and not for the shipped
  binaries. `crates/launcher-gui/tests/dependencies.rs` states the exception
  with its one cause. Decide whether the toolkit dependency is acceptable, or
  whether the assets crate can be dropped, before a release claims otherwise.
- Better Launcher's gesture activation now has a route: the Better Touchpad
  gesture pipeline opens it over `org.betteros.Launcher1` when the thumb-and-
  three pinch is recognized. Better Defaults still does not apply its keyboard
  shortcut; ticket 27 owns that.
- The GJS exception was granted on 2026-08-31 and ticket 38 spent it. The
  adapter extension is three files, the language rule above states its bounds,
  and ADR 0012's amendment records what was granted, which bound had to be
  widened, and why. The one widening: the interface serves four methods rather
  than only signals, because a signal-only extension can report a gesture and
  cannot open the overview.
- The gesture path is two processes, and neither is enabled by installing.
  `touchpad-adapter@betteros.org` is a GNOME Shell extension the user enables,
  and `better-touchpad-gestures.service` is a user unit that is installed and
  not enabled. Without both, Better Touchpad configures gestures and performs
  none, and the Gestures screen and the `touchpad.gesture_bridge` health check
  both say which is missing.
- Three rows of the Mac-style preset do not work on GNOME 46 and must not be
  described as if they do. Four fingers down maps to the current application's
  windows and there is no shell facility for it. The two-finger application
  gestures reach no adapter. And nothing animates: the recognizer sees
  continuous progress and the actions fire at the end of the gesture, because
  GNOME 46 exposes no way to drive its own overview transition from outside.
- The GNOME Shell adapter has been run against a real shell exactly once, on a
  nested GNOME Shell 46.0 in an isolated session: it enabled, owned its name,
  found both of the shell's swipe trackers, suppressed and restored them, and
  performed every action method. Its *event* path has not been seen on a live
  shell, because a nested shell with no touchpad produces no gesture events, so
  the swipe and pinch stream is tested only against a Rust fake of the same
  interface. Do not claim the event shape has been observed against a hand.
- `ingest::EventScale::swipe_pad_pixels` is 1000, and it is a recorded starting
  value in the same sense as the thresholds: a compositor reports swipe motion
  in pixels and never says how big the pad is. Nobody has tuned it.
- Write the security review the libinput gesture path needs, or record that it
  will not be written. ADR 0008 made it a precondition and ADR 0012 keeps it: a
  user-session process that can read the touchpad can read the keyboard, and
  that risk needs an owner and a document, not a paragraph in a pull request.
- `touchpad_gestures::conflict::GNOME_46_GESTURES` is a static model of GNOME
  46's own swipe trackers, not a probe — the shell compiles them in and exposes
  nothing to read. Both trackers accept three *and* four contacts, which is why
  the Mac-style preset collides with four of them. Re-check the model before
  claiming support for a newer GNOME, and produce the hardware and release
  matrix ADR 0008 asked for; nothing here has been tested against GNOME 47, 48,
  or 49.
- Better Touchpad's gesture thresholds are recorded starting values, not a
  tuned curve. Activation 0.6, cancellation 0.25, cooldown 350 ms, and the
  travel distances in `RecognizerScale` have never been tried against a hand,
  and Issue #3 defers the curve. The recognizer's 104–126 ns per frame is over
  replayed synthetic frames; the live pipeline's frames come from the adapter
  and have never been produced by a hand, so there is still no end-to-end
  gesture latency to quote.
- Better Touchpad's gesture profiles are per device now, and its pointer,
  scrolling, and clicking settings are still session-global. ADR 0010 records
  which half is which and why. `gestures.json` is schema version 2 and migrates
  version 1 on read; the capture beside it is still one file for the machine
  rather than one per profile, so a restore puts the capture back into whichever
  profile is in force. And nothing routes a recognized gesture to the profile of
  the pad that produced it. `better-touchpad-gestured` recognizes against the
  profile in force — the selected pad's own, or the global one — rather than
  always the global one, but `org.betteros.TouchpadAdapter1` reports no device
  identity, so on a machine with two touchpads both pads perform the selected
  pad's profile. Routing per pad needs the adapter interface to name the device
  first, which is an ADR 0012 change rather than a GUI one.
- The custom-shortcut collision check reads only what a dconf database holds,
  which is what the user has changed. GNOME's compiled-in defaults are in no
  database and the shell exposes nothing to ask, so the screen says "nothing you
  have changed uses this" rather than "no conflict". A recorded binding spelled
  outside `better-actions`' fixed key table — a media key, a keypad key — cannot
  be compared and is counted rather than guessed at, so the check can miss a
  collision with one of those. Decide whether a wider key table is worth the
  execution surface before treating the note as complete.
- Decide how deep two-finger application support goes. The Mac-style preset
  carries application back, forward, zoom, and rotate because Issue #3's table
  does, and every adapter reports all four unsupported. A four-row preview that
  says "no backend can do this yet" is honest but it is not a feature.
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
- Better Files reads device state from `org.betteros.Storage1` through
  `storage_service::StorageClient`, and runs the same `storage-core` state
  machine in its own process when that service is absent. The window says which
  it got, and the in-process case is drawn as a warning rather than a note,
  because an in-process engine never sees the tracked-operation notices another
  application would have sent to a service. Do not add a third fallback that
  invents a state.
- `files-operations` does not yet tell the storage service when a job finishes.
  `StorageClient::notify_operation_completed` exists and is tested against a
  running service; the job engine has no device identity for a destination path,
  so nothing calls it. Until a path is mapped to a UDisks2 object, a Better Files
  copy to an external device reaches the service only through the platform
  signals, not as a tracked operation — which means readiness can be claimed
  earlier than it should be for our own writes. Close this before claiming Issue
  #5's readiness rule is fully implemented.
- Better Files cannot turn Performance mode on. The client can set the policy and
  the service refuses it without the acknowledged risks, but no UI presents those
  risks, and Issue #5 requires the trade-off explained before activation.
- Preview treats a parser as a boundary, not a sandbox. The size limit, the
  decoder's own allocation limits, and a `catch_unwind` are what exist;
  `docs/files-preview-policy.md` states what each one does and does not buy. A
  real sandbox means a separate process with a seccomp profile, and it is the
  right shape before preview grows beyond image and text.
- No Better Files performance claim may name Nautilus, COSMIC Files, or Windows
  File Explorer. `cargo bench -p files-gui --bench files_suite` measures Better
  Files against itself on one machine with a warm page cache, and the copy
  figures are page-cache figures. The comparison hardware and datasets are Issue
  #6's own deferred decision and need one before any such claim.
- Nothing enforces the benchmark budgets `components/manifests/better-files.yaml`
  declares. There is no CI job that runs the harness and compares.
- Nothing enforces the benchmark budgets the shipped manifests declare. Better
  Files and Better Launcher both now have harnesses that produce every number
  their manifests name, and neither has a stored baseline or a CI job that runs
  the harness and compares against the declared maximum regression. A budget
  nobody checks is documentation, not a gate.
- Decide whether `app-chooser-core` should offer a typed "remove this
  association" operation. Without one, restoring an XDG default that previously
  had no owner reports Manual action required rather than clearing the line, and
  no second `mimeapps.list` editor may be written.
- Capture a handler group's previous value per declared type. A group whose
  types currently point at different applications reads as unknown and is
  refused rather than flattened into one owner, which is safe but coarse.
- Better Files runs its operations as durable jobs now: `files-operations` owns
  copy, move, duplicate, rename, bulk rename, trash, restore, permanent delete,
  and checksum, and a job survives every handle to it being dropped. Four gaps
  are open and should not be assumed closed. Archive and extract are not built.
  The trash has no per-device `.Trash-$uid`, so a deletion on a removable disk
  copies into the home trash. Hard links are not preserved between separately
  copied files. And a job record is a full rewrite rather than a journal, which
  is 17.5 MB at 10,001 items and grows linearly, so a job of a million items
  needs an append-only item journal first.
- Decide whether a Better Files job should survive a logout or a reboot, and
  where the Better Copy boundary sits. Issue #6 defers both; persistence today
  covers a UI restart only.
- Measure Better Files copy performance against real hardware before claiming
  it. Every published number is a page-cache number from an ext4 temporary
  directory: no spinning disk, no USB device with `fsync` per file, and no
  network share has been measured.
- The built-in catalog now carries all seven released component manifests, so
  `better-manager list` offers the whole suite and `better-manager defaults
  inspect` reports Better Files' declared file-manager integration rather than
  an empty state. Two gaps stay open. `better-files-example.yaml` is still in
  the shipped catalog beside the real `better-files`, which shows a user a
  fixture; removing it means moving the `better-files-example` entry in
  `manager-gui`'s `translated_component` and its GUI test, which is a code
  change rather than a list change. And Better Files is the only component that
  declares a default integration; the other six declare none, which is a
  deferred Issue #10 decision and not an omission this catalog change made.
