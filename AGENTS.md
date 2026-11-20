# Better OS Agent Contract

Better OS is a modular performance-improvement layer for Zorin OS and Ubuntu.
It is not a Linux distribution fork. First-party components are installed,
updated, verified, and rolled back through shared manager operations.
On 2026-11-21 the owner chose a Better OS desktop session on COSMIC's
compositor beside GNOME, which stays installed and the default until that
session meets its bar
([ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md),
`engineering/specs/better-os-session.md`).

## Required artifacts

Read `AGENTS.md` before doing any project work; it is the project operating
contract.

- `engineering/README.md` — index of the engineering documents.
  Read it first on arrival.
- `engineering/handoff.md` — unfinished work that has no ticket. It exists only
  while there is some. Read it on arrival, and write it when work without a
  ticket stops unfinished.
- `engineering/architecture.md` — how the system is built. Read it before
  changing crate boundaries, public contracts, test seams, packaging,
  privileged-operation design, or the system decomposition.
- `engineering/decisions/` — why hard-to-reverse choices were made. Read the
  relevant record before proposing to change such a choice; a later decision
  gets a new record.
- `engineering/specs/` — what each feature or policy promises and how to tell it
  is done. Read the one for the feature before working on its tickets, and
  update it before the code when a requirement changes.
- `engineering/contracts/component-manifest.md` — the manifest schema. Read it
  before changing manifest fields or validation rules.
- `engineering/performance/` — benchmark methods and recorded numbers. Read the
  file for a component before making or changing a performance claim.
- `engineering/tickets/` — the work, one slice per ticket. Open tickets are at
  the top level, tickets in progress in `doing/`, finished ones in `done/`; the
  directory is the ticket's only state. Pick up an open ticket whose blockers
  are all in `done/`, and move it to `done/` in the change that delivers it.
- `docs/` holds only documentation for users: `docs/third-party-licenses.md`
  ships in every package. No engineering document goes there.

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
- Every Better OS background service runs because it was installed, with
  nothing for the person to type (owner, 2026-11-12 and 2026-11-13). A new
  service, tray, or hook is enabled and started by the install step, comes
  back at login, can be started on demand by D-Bus activation where a client
  asks for it. The only things that stop it are disabling the component in
  Better Manager (it stays off, across logins, until enabled again) and
  uninstalling it; the record decides, so a unit someone stopped by hand is
  started again by Better Manager. A component whose service needs a manual
  command is not done. Uninstalling removes everything the component added:
  its services and their enablement, autostart entries, D-Bus activation
  files, udev rules and sleep hooks, GNOME extensions it enabled, keybindings
  and defaults it set (restored to what they were), and its caches. Nothing
  it installed outlives it. The person's own data (rules, history, recent
  lists, trust lists, per-folder views) is deleted too unless they untick
  "Also delete my settings and history", a check box in the uninstall
  confirmation that is ticked by default; unticked, it stays for a reinstall.
  Each manifest's `paths` names that data so the manager knows what to
  delete.
- Everything Better OS offers is complete after one action in Better
  Manager (owner, 2026-11-16): installing a component, turning a feature on,
  or switching something over needs no terminal command, no further setting,
  and no second step the person has to know about; undoing it is one action
  too and leaves the machine as it was. A feature that works only after a
  manual step is not done.
- A component's systemd **user** units are managed by `manager-core`, never by
  a window or a command directly. `RealDriver` takes the controller through
  `with_user_units`; a driver built without one manages no units. Enabling,
  starting, disabling, and stopping them, and the startup pass that makes an
  existing record true (`Manager::heal_user_units`), go through
  `manager_platform::user_units::UserUnitController`. Units come from dpkg's
  own file list under `/usr/lib/systemd/user`, never from a table by component
  id, and the health check requires an enabled component's units to be enabled
  and active. An exited unit, even one that exited 0, is not running, and the
  startup pass and Check again start it like a stopped one (ticket 131). Units
  that still exit 0 are the Awake tray's and the Monitor tray's Quit and
  `better-window-attention` finding another instance on its name; the trays
  therefore come back the next time Better Manager starts or on Check again,
  which is this rule (only disabling or removing keeps a unit off). The
  touchpad gesture service, which used to exit 0 with nothing configured,
  stays resident and idle. Update and
  Restore restart a unit that is already running so the new binary runs; Verify
  starts a stopped unit before checking. Tests set
  `BETTER_OS_TEST_SYSTEMCTL_GUARD` so the real runner can never reach the
  person's own user manager. User units need no privilege, so this never crosses
  `manager-daemon`; system units are out of scope.
- After any update of a Better OS package, however it was installed (Better
  Manager, `apt upgrade`, Software Updater, unattended upgrades), every Better
  OS process of that package runs the new program within a few seconds:
  services, tray icons, and open windows (owner, 2026-11-14, ticket 105). No
  process may keep running a `(deleted)` executable. Three mechanisms carry it,
  and a new process belongs to one of them:
  - A package that ships a systemd user unit gets the preinst and postinst
    `packaging/build-deb.sh` builds from `packaging/maintainer/user-units.sh`:
    for every logged-in person it reloads their manager, `try-restart`s the
    package's units (never starting one that is stopped), and enables a unit
    new in that version only for a person who had the component's other units
    enabled. Better Manager's own restart after an update stays; both only
    restart what runs, so they never disagree. `verify-deb.sh` checks that
    every package shipping a user unit carries the scripts.
  - A process with no window — a service or a tray — is a systemd user unit,
    and the first mechanism restarts it; it does not watch itself. (Both
    trays became user units in tickets 97 and 102; the Awake tray's own
    exec-restart from ticket 105's first draft was removed for that.)
  - Every window mounts `better_ui::restart::UpdateRestart` and implements
    `Restartable`.
    The program watches its own file, waits for `dpkg-query` (never the
    status file alone, which is stale while dpkg runs) to report the new
    version configured, saves its view (page, tab, selection, scroll, window size,
    anything being typed) into a private hand-off in
    `$XDG_RUNTIME_DIR/better-os/restart`, and `exec`s the new program in the
    same process, which reads the hand-off and shows "Updated to {version}".
    While it is in the middle of work it cannot carry over — a file
    operation, a Git command, a package transaction, a dialog whose typed
    input has no saved form — it says "Updated — restarting when this
    finishes" and waits. What blocks a restart and the view's shape live in
    each window's GPUI-free `restart` module, with tests. Every toast a
    window shows goes through `better_ui::fluent::toast` and the window's
    one `ToastHost` (`UpdateRestart::toast_host`); there is no second toast. A window built by a
    test or a benchmark uses `UpdateRestart::inactive` and never restarts.
  - `better-manager-daemon` is never stopped or restarted from inside its own
    unit, where systemd's stop kills the dpkg run executing the maintainer
    script and leaves the package half-installed (reproduced in a VM with the
    0.2.12 prerm). Its prerm stops it only on removal, its postinst
    `try-restart`s it after an upgrade from outside unless that daemon is in a
    transaction, and a daemon whose
    program was replaced leaves ten seconds after its last transaction so
    D-Bus starts the new one. It leaves by refusing new calls with the
    retryable `daemon.error.shutting_down` (the client asks again), letting
    the calls inside finish, and only then giving its bus name up: releasing
    the name of a `Type=dbus` unit first makes systemd stop it at once. Better Manager updates the daemon as a
    companion of its own package (ADR 0028, ticket 124), and never inside
    the daemon's unit when the installed prerm could stop it: from 0.2.16 the
    daemon installs its own package last, with `systemd-run --wait` in a
    transient unit; 0.2.13 to 0.2.15 install it as their own last step under
    the scripts above; 0.2.12 and older are never sent it, and the Better
    Manager they install starts `/usr/libexec/better-os/daemon-handoff` in a
    transient unit, which waits for the old daemon to go idle and installs the
    staged daemon and adapter from outside it. A plan whose companions the
    daemon cannot take is refused as `daemon.outdated`, never resent without
    them. `SELF_UPGRADING_SINCE` and `HANDOFF_SINCE` in
    `manager_core::exec` name 0.2.16 and move with a version bump that is not
    0.2.16.
- Use Rust for first-party production code. Use Go only after recording a
  concrete reason in an ADR. Do not add C, C++, Python, JavaScript, Electron,
  Tauri, or GTK application code. **One bounded exception exists**, granted by
  the project owner on 2026-08-31 and recorded in ADR 0012: GJS is permitted
  inside the Better Touchpad GNOME Shell adapter extension
  (`adapters/gnome-shell-touchpad/`) and inside the shared Better OS shell
  adapter extension (`adapters/gnome-shell-adapter/`, ADR 0018, proposed), and
  nowhere else. Inside them, JavaScript may only bridge typed events and typed
  actions — reporting what the compositor saw, and invoking something GNOME
  Shell already exposes. It may hold no threshold, no cooldown, no cancellation
  rule, no conflict decision, no configuration, no per-application rule, and no
  decision of any kind, it may store nothing, it may execute nothing, and no
  method it serves may take free text. Each relay of the shared extension, and
  the touchpad adapter's suppression of GNOME's swipe trackers (ADR 0035,
  proposed), acts only while its Rust counterpart owns its bus name, answers
  only that owner, accepts the owner only when its process runs the packaged
  service, and otherwise leaves GNOME's own behaviour in place. `crates/touchpad-session/tests/extension.rs` and
  `crates/window-attention-service/tests/extension.rs` assert those bounds
  against the files. Widening them needs a new ADR, not a commit.
- Use GPUI with `gpui-component` for first-party desktop GUIs. `better-ui` owns
  shared presentation primitives.
- Treat manifests as untrusted input: validate schema, targets, artifacts,
  dependencies, conflicts, and lifecycle metadata before planning.
- Do not add a public APT repository, signing implementation, or automatic
  optimizer without an explicit decision. The one decision so far (owner,
  2026-11-21, ADR 0038 C2): `manager-daemon` may add System76's
  `apt.pop-os.org` source with its one signing key and a pin that takes only
  the Better OS session's package closure from it, all compiled into the
  daemon, and removes them with the session. Any other third-party source,
  and an APT repository or signing implementation of Better OS's own, still
  needs its own decision. The project license (ADR 0003) and
  the privileged daemon IPC protocol (ADR 0007) are decided; changing either
  needs a new ADR.
- Every behavior change needs tests. Run formatting, linting, workspace checks,
  and tests before handoff.
- CI runs clippy with the newest stable toolchain, which can be ahead of the
  local one and reject code it accepts (a duplicated `#[allow]` attribute, new
  lints). Run clippy with the newest stable available, read the main CI run
  after every push to main, and fix a red main before the next merge.
- A test never executes a file it has just written: another test thread's
  fork can still hold it open and the exec fails with ETXTBSY. Run a written
  script as `/bin/sh <script>`, or reach a checked-in fixture through a
  symbolic link the test makes (`crates/files-gui/tests/share.rs`).
- A test that watches a file never writes that file from its own process:
  another test thread's fork holds a copy of the write descriptor until its
  exec, and that late close reaches a watch started afterwards as a
  close-write. Have a child write it (`install` in
  `crates/better-core/src/self_update.rs`).
- Every sandbox desktop a test or a screenshot starts (headless GNOME Shell,
  sway, `dbus-run-session`) runs under
  `systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0`, with a
  no-op `ibus-daemon` (a script that execs `sleep infinity`) first on its
  `PATH`, and is ended by PID when done. A sandbox GNOME Shell otherwise starts
  its own `ibus-daemon`, which has grown to 12 and 21 GB, and the kernel's OOM
  kill took the owner's Claude Desktop down with it twice (2026-11-14, -15).
- A test module inside a file that does `use gpui::*` imports what it needs
  by name, never `use super::*`: the glob brings gpui's own `#[test]` macro
  in, which shadows the standard one and fails to compile with a recursion
  limit error (seen in tickets 123 and 127).
- Give every worktree of this repository its own `CARGO_TARGET_DIR`. Cargo
  hashes a path crate by its path inside the workspace, so two worktrees sharing
  one target overwrite each other's artifacts and a stale build from the other
  tree is judged fresh.
- `rust-version` in the workspace manifest is the oldest toolchain that builds
  the locked graph. A change that needs a newer compiler raises it in the same
  commit; the CI `msrv` job checks the workspace with exactly that version.
- A rule that two places need (reading `/etc/os-release`, probing the host)
  lives in one place both call. A fix applied at one call site of a rule copied
  elsewhere is not a fix; find every copy first.
- A fallback that turns "unknown" into a plausible value is a defect: when a
  value is missing, show that it is missing rather than a default that reads as
  a fact.
- A container check runs without network: anything APT needs at test time is
  installed into the image when it is built.

## Handoff

Before handoff, put each ticket in the directory that matches its real state,
keep `## Notes` current on each ticket in progress, give a blocked ticket its
`**Blocked:**` line, write unfinished work that has no ticket to
`engineering/handoff.md`, and state which checks actually ran. Do not claim a
GUI or dependency compiles when the relevant command was not executed.

## Follow-ups

- Build each supported Ubuntu release in a compatible base environment. The
  current Zorin 18 host produces `libc6 (>= 2.39)` and must not supply a 22.04
  release artifact.
- `engineering/specs/copyleft-review.md` reviews every copyleft dependency and leaves four
  decisions to the owner: which FreeType license the Ubuntu 22.04 windows use,
  shipping the Apache-2.0 text (and the permissive licenses' texts generally),
  where the dependencies' Corresponding Source lives, and the license of two Zed
  crates with no license field. Until they are decided, the published packages
  carry the GPL text and a license inventory and nothing else. Re-run the
  review when `Cargo.lock` gains a copyleft package.
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
  metrics; the reasoning is in `engineering/specs/monitor-collector-sources.md`.
- Decide the package signature format before offering a signed distribution
  channel. Checksums are currently the only integrity mechanism.
- The daemon repairs a dpkg run a crash interrupted (ADR 0014): a method with
  no argument, its own polkit action, `dpkg --configure -a` and nothing else.
  It has only ever run against the fake driver. The daemon's systemd sandbox
  (`ProtectKernelModules`, `DevicePolicy=closed`, and the rest) is inherited by
  dpkg's maintainer scripts, so a repair after an interrupted kernel upgrade may
  fail where the same command in a terminal succeeds; ADR 0014 says how to test
  that in a VM. The failure message names the terminal command.
- The manager strips a dpkg epoch before it compares versions, so `1:0.2.8`
  reads as older than `0.3.0` while dpkg orders it the other way. An update
  planned from such a version would be a downgrade to apt.
- Better Launcher is measured now: `cargo bench -p launcher-gui --bench
  launcher_suite` runs all five manifest benchmarks and
  `engineering/performance/launcher-performance.md` records the numbers and the hardware. Two
  limits travel with them. `warm-overlay-open` ends at the first renderable
  model, because `ZED_HEADLESS=1` has no compositor and therefore no frame — no
  Better OS component has a time-to-photon figure. And the idle CPU figure is a
  headless one: nothing asks the window to repaint, so it is the launcher's own
  idle cost and not a claim about a launcher on a running desktop.
- All ten packages are published. `v0.2.16` carries every component
  `packaging/build-deb.sh` builds, for Ubuntu 22.04 and 24.04 on amd64 and
  arm64, and every shipped manifest now records the checksum of its own
  published 0.2.16 asset, verified after re-downloading the public release. No
  first party manifest carries a placeholder checksum any more;
  `components/manifests/better-files-example.yaml` is the one exception and is a
  schema fixture, not a released component. A version bump puts the placeholders
  back for the length of the release branch, because the checksums a manifest
  carries describe the previous release's packages and not the ones about to be
  built — write the real values back only after the new release is public and
  its assets have been downloaded again, and correct the comment above each
  block at the same time rather than leaving it describing the release before.
- Better Git is the ninth package (ticket 78), first published in v0.2.9, so a
  release now carries 36 `.deb` files, 36 `.deb.sha256` sidecars, `LICENSE`,
  and `third-party-licenses.md`: 74 assets, where v0.2.8 had 66. Its
  manifest declares `component_type: enhancement` with nothing in `replaces` or
  `enhances` and `icon: generic`, because both sets are closed and a released
  Better Manager refuses a value it does not know; a standalone type is a schema
  change for its own ticket. The desktop entry is
  `io.betteros.Git.desktop`, named after the window's `app_id` as
  `better_ui::window_options` requires; `verify-deb.sh` and
  `crates/git-gui/tests/manifest.rs` both hold the two together.
- `packaging/build-deb.sh` clears `dist/` of the previous build's `.deb` and
  `.deb.sha256` before it builds, and must keep doing so. `verify-deb.sh` picks
  each package with a version-agnostic glob and fails when that glob matches
  more than one file, so a bump used to leave the verifier failing on a stale
  package with a message that named the component rather than the leftover. Only
  the top-level package files go; the scratch subdirectories under `dist/`
  belong to other tools and must survive.
- Every Better OS window draws its own frame, because Mutter offers no
  server-side decorations to an `xdg-toplevel` client — without it a window has
  no close button and cannot be dragged or resized. Since ticket 108 it is the
  desktop's frame: open the window with `better_ui::window_chrome::window_options`
  and `window_chrome::root`, and put `fluent::chrome::TitleBar` first in the
  view. `better_ui::system_frame` dlopens GTK 4 (and libadwaita) at run time and
  has it draw the frame, shadow, header, and window buttons from the current
  theme, as Chromium does; without GTK the kit draws Adwaita's frame. Do not
  paint a background on a view's root (the frame paints it with the desktop's
  corners), and do not draw frames or window buttons in an app. The frame was
  compared pixel by pixel with GNOME Text Editor in a headless GNOME Shell 46
  only, not on the owner's desktop. Better Launcher is the one deliberate exception, recorded
  in code: a Start-menu panel (ticket 82) dismissed by Escape gets no titlebar, but
  it still sets `app_id` like every other window, which is what lets a dock or
  an application grid match a window to its desktop entry. Escape on Better
  Launcher is now a tested key mapping and was pressed in a headless sway
  session, where it closed the overlay; it has still not been pressed on a GNOME
  desktop. The icon half of that pair moved in v0.2.4 without closing:
  every icon v0.2.3 shipped was in fact invisible, because gdk-pixbuf chooses a
  loader by sniffing a file's first bytes and the attribution comment pushed
  `<svg>` past that window — a valid SVG document that was not an image to the
  desktop. `verify-deb.sh` now asserts both properties, that the named icon file
  ships *and* that `<svg` appears inside the first 100 bytes, and all six were
  rendered through the host's own gdk-pixbuf. That is a loader accepting the
  file, not GNOME drawing it in a grid, so the desktop confirmation is still
  owed. Also note that GPUI's window icon is X11-only and takes a raster image,
  so on Wayland the icon comes from the desktop entry via `app_id`, not from the
  shipped SVG.
- A published binary embeds the manifests as they stood when it was built, and a
  manifest can only record a real checksum after its own release is public. So
  the `better-manager` shipped in a release always carries the pre-release
  placeholder checksums for that release. That is now a fallback rather than the
  only path: ticket 41 and [ADR 0013](/engineering/decisions/0013-remote-catalog-refresh.md)
  made the catalog refreshable, so a manager fetches the eight manifest files
  from `main` over HTTPS, validates them with the same validator the compiled-in
  ones get, caches the result in the state directory, and shows the built-in
  catalog with a visible "may be outdated" state when it cannot. The placeholder
  state the fix targets has now been observed rather than only reasoned about:
  the real-network catalog test was run from a build of the v0.2.2 merge commit
  `b5f6e34`, whose embedded catalog carries that release's placeholders, and it
  refreshed from `main`, planned `better-monitor` 0.2.2, and verified the real
  published `.deb` against the fetched checksum. That run has not been repeated
  since; v0.2.3 through v0.2.8 changed nothing about the mechanism,
  so the observation stands for the path and not for any later release's own
  binary.
  Two limits still travel with it.
  Signing is still deferred, so HTTPS plus
  the artifact checksum is the whole integrity story and a rolled-back-then-
  re-bumped `main` is indistinguishable from a real release. And there is no
  scheduled refresh: it happens once at launch, on either of the two buttons
  that ask for one — the Components screen's list refresh and the Updates
  screen's check, which call the same method — and on `better-manager catalog
  refresh`, so an install planned from a window that has been open for a week is
  planned from a week-old catalog whose age is on screen.
- `install.sh` at the repository root is the first install path for a fresh
  machine and ships only `better-manager` and `better-manager-daemon`; the other
  six are installed from inside Better Manager. It has no metadata to read but
  ADR 0002's asset names, so any change to the naming contract or the supported
  release/architecture matrix must change the script's tables in the same
  commit. `engineering/specs/release-packaging.md` holds the contract.
- The CI `rust` job now runs `packaging/generate-third-party-notices.sh --check`
  before anything is built. `docs/third-party-licenses.md` pins the `Cargo.lock`
  hash, so any lockfile change makes it stale; regenerate it in the same commit
  that moves the lockfile rather than letting the check find it.
- `better-monitor` ships the window, the session service, the command line, and
  a systemd user unit in one package. v0.1.0 published the window alone, so the
  wider package carries its own version: it moved to 0.2.0 and travels with the
  workspace since. One version number no longer names two payloads.
- `better-touchpad` now has a manifest. Its health checks are the IDs
  `touchpad-core` emits and its benchmark baselines are the figures in
  `engineering/specs/touchpad-sensitivity-mapping.md`, but nothing runs those benchmarks —
  the same unenforced-budget gap `better-files.yaml` carries. Its checksums are
  the published v0.2.16 ones.
- A package installs its systemd user unit and does not enable it, matching
  `better-manager-daemon`; Better Manager enables it (ticket 79). Nothing in
  dpkg stops a running user service at removal either; Better Manager stops it
  before the package goes, and `better-awake.yaml`'s
  release notes describe behaviour the manager owns rather than the package.
  The one thing dpkg does to a user unit is on an upgrade (ticket 105): the
  postinst restarts the units that run and enables a unit that is new in that
  version for a person who had the component on, so `apt upgrade` never
  leaves a tray that replaced an autostart entry switched off.
- Better Awake detects nine of Issue #13's eleven trigger kinds. Fullscreen
  needs a compositor adapter and reports itself unavailable; audio reads ALSA and
  cannot see Bluetooth or network sinks. Both limits are recorded in ADR 0010 and
  in the provider modules. Do not claim full trigger coverage.
- A Better Awake low-battery stop raises one desktop notification: from the
  tray when it owns `org.betteros.AwakeTray1`, from the service otherwise. Two
  one-round-trip races remain, a tray starting or quitting at the moment of the
  stop, and the service's notification follows the user manager's `LANG`,
  which can differ from the desktop's. None of it has been seen on a GNOME
  desktop; the proof is a fake notification service on a private bus.
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
- The gesture path is two processes, and installing turns both on (ticket 88).
  `better-touchpad-gestures.service` is a user unit like any other, and
  `touchpad-adapter@betteros.org` is a GNOME Shell extension that
  `manager_platform::shell_extensions` adds to the person's
  `org.gnome.shell enabled-extensions` on install, enable, update, restore,
  and verify, and takes out on disable and removal, leaving every other entry
  alone. Like units, which extensions a component owns comes from dpkg's file
  list under `/usr/share/gnome-shell/extensions`, the startup pass turns an
  enabled component's extension back on, and tests are kept off the person's
  settings by `BETTER_OS_TEST_SYSTEMCTL_GUARD`. A running shell loads a newly
  installed extension only at the next login, so one login after install is
  still needed. A package may ship `/usr/lib/better-os/before-removal/<package>`:
  manager-core runs it as the person (only a regular, root-owned file nobody
  else can write) after the units stop and right before the daemon removes
  the package, and with `--undo` if the daemon then refuses. It never blocks a
  removal: exit 3 is a warning in Activity, exit 4 means no session and a
  pending restore Better Manager finishes at its next start. Better Touchpad's
  puts its captured touchpad settings back. The gesture service's first run puts the Mac-style preset in
  force when no gesture file exists yet. Without both processes the Gestures
  screen and the `touchpad.gesture_bridge` health check say which is missing:
  the service owns `org.betteros.TouchpadGestures1` while it runs, and an
  adapter on the bus with that name unowned is "not running", never
  "connected" (ticket 131). The service waits for the adapter instead of
  exiting without it (a login starts it a moment before GNOME Shell enables
  extensions), never exits on purpose (with safe mode on, nothing configured,
  or the integration turned off by itself it stays resident and idle), and
  ends a SIGTERM by that signal after giving GNOME its gestures back. It
  watches `gestures.json` and the safe mode marker and takes every change at
  once, and a watch that cannot be made costs no gesture (it says so and is
  retried every minute). The service and both windows follow the file and
  write only their own changes onto it as it is now, through one merge under
  one lock (`GestureStore::save_changes`). Its match rule names the adapter as
  the signals' sender, and on attach it adopts what the adapter reports, so a
  suppression an older extension and service left behind is lifted. GNOME's
  own swipe trackers are suppressed only by the service, only for a gesture
  that collides, is on, and verified, and only once a gesture has arrived from
  the adapter; the extension takes that request only from the service name's
  owner running `/usr/bin/better-touchpad-gestured` (ADR 0035, proposed) and
  restores the moment the name loses its owner. A companion package's
  extension (`shell-adapter@betteros.org` from `better-shell-adapter`, a
  companion of `better-manager`) is turned on with the component it travels
  with, read from the companion's own dpkg list. GNOME's switch that turns off
  all extensions is a warning, never a failed check, for every component.
- Suppressing GNOME's three-finger swipes is not reliable on GNOME 46 and must
  not be described as if it were (ADR 0035). `workspaceAnimation.js` turns its
  tracker back on after every animated workspace switch and when the overview
  hides, and the overview's workspace strip and application grid have
  three-finger trackers the adapter never touches, so a Better OS gesture on
  three fingers can fire beside GNOME's. The Touchpad page says so when such a
  gesture is on; the Mac-style preset uses four fingers and collides with
  nothing.
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
  The extension's watch on `org.betteros.TouchpadGestures1` and its refusal
  of a suppression from anyone else (ticket 131) have not run in a shell
  either; they are asserted against the file, and the service side against a
  private bus.
- `ingest::EventScale::swipe_pad_pixels` is 1000, and it is a recorded starting
  value in the same sense as the thresholds: a compositor reports swipe motion
  in pixels and never says how big the pad is. Nobody has tuned it.
- Write the security review the libinput gesture path needs, or record that it
  will not be written. ADR 0008 made it a precondition and ADR 0012 keeps it: a
  user-session process that can read the touchpad can read the keyboard, and
  that risk needs an owner and a document, not a paragraph in a pull request.
- `touchpad_gestures::conflict::GNOME_46_GESTURES` is a static model of GNOME
  46's own swipe trackers, not a probe — the shell compiles them in and exposes
  nothing to read. Both trackers take three contacts and only three
  (`swipeTracker.js`: `GESTURE_FINGER_COUNT = 3`, read from GNOME Shell 46.0's
  gresource on 2026-11-21, ticket 131), so the Mac-style preset, which swipes
  with four, collides with none of them. The model used to say three *and*
  four, and 0.2.16 took GNOME's three-finger gestures away for nothing; the
  service now re-detects collisions on every start rather than trusting the
  annotation a file carries. Re-check the model before
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
  profile is in force. Each gesture is routed to the profile of the pad that
  made it (ADR 0015): the adapter reports the event's device node and the
  service resolves it to the stored identity, with an unknown device using the
  global profile. No gesture from a real touchpad has been observed, so which
  device Mutter reports as an event's source is still read from its source code
  rather than seen.
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
- Better Files becomes the desktop's file manager (ticket 129, ADR 0041) by a
  user-level D-Bus activation file for `org.freedesktop.FileManager1` that
  runs `better-files --service`; a running owner is never stopped, and
  removal runs `/usr/lib/better-os/before-removal/better-files` to put the
  previous file (or none) back. The routing has run only on a private
  `dbus-daemon` with stand-in file managers: nobody has watched a real
  "Show in folder" open Better Files on GNOME, nor a real Nautilus give the
  name up, and the adapter's executable table is the only thing that may
  name a program for a bus name.
- Better Defaults writes GNOME keybindings and desktop settings through the
  dconf service now, and the write has only ever reached a real `dconf-service`
  on a private bus. Nobody has watched GNOME Shell or Settings pick up a change
  it made. No shipped manifest declares a GNOME integration yet, so the path is
  unreachable from the built-in catalog until one does.
- Better Touchpad's scroll factors are live on GNOME 46 through the scroll
  pass-through (ticket 125, ADR 0030): `better-touchpad-scroll@eventN.service`,
  one per touchpad, started by udev, grabs the touchpad's evdev node only while
  a factor other than 1 is set and re-emits it through a uinput clone with
  two-finger motion scaled. It is the only first-party code that opens
  `/dev/input` or `/dev/uinput`; its security review is
  `engineering/specs/touchpad-input-passthrough-security.md`, and a change to
  the device rule, the unit's sandbox, or the bus interface updates that review
  in the same commit. Nothing has run against a real touchpad yet:
  `packaging/tests/touchpad-scroll-hardware-check.sh` is the owner's check.
  Smooth scrolling stays unavailable, because it is each application's drawing
  decision and not an input setting.
- The keyboard's colour (ticket 136, ADR 0034) is set through
  `manager-daemon`, for the built-in keyboard only (an I2C LampArray, or
  asus-wmi's `kbd_rgb_mode`; USB and Bluetooth LampArrays are out of
  scope): a LampArray by a helper unit whose private `/dev` holds its one
  hidraw node and which drops to `nobody` once it is open (`--lamp-array`),
  `kbd_rgb_mode` by ADR 0029's attribute helper. Both helpers share
  `hardware::helper_sandbox()`; `ProtectKernelTunables=`,
  `ProtectControlGroups=`, `ProtectProc=`, and `PrivateNetwork=` are left
  out on purpose, because each brings the whole of `/sys` back (ADR 0034;
  `IPAddressDeny=any` closes the network instead). No session
  process opens a hidraw node and no udev rule grants one;
  `hardware_core::lamparray` is the only parser of report descriptors and
  `manager_daemon::keyboard_lighting` the only code with hidraw ioctls. The
  vendor report 0x5F and the 0xFF59 copy are never written. CI's
  `packaging/hardware-unit/check.sh` runs both helpers' units on a real
  systemd in a container with the property lists the release daemon prints
  (`--helper-properties`), but has no hidraw device, so no feature report
  has gone through the sandboxed helper; the
  lamp count, `LampArrayKind`, and level counts of the owner's M5406WA,
  whether its intensity and the WMI backlight levels 0–3 multiply or
  override each other, and whether the colour survives suspend are unread.
  Level counts follow HID Usage Tables 1.4 section 26.9 (N is off plus N
  levels). Ticket 136's notes hold the owner's check, done on a released
  build through Better Manager. No LampArray effect is offered, by design;
  only `kbd_rgb_mode`'s four firmware effects are, and that path has run
  against fixture sysfs only.
- Removing `better-manager` or `better-manager-daemon` gives the hardware
  back (`better-manager-daemon --release-hardware` in both prerms): the
  keyboard's colour as above, and the battery's charge limit to what it
  read before Better OS's first write, recorded in
  `/var/lib/better-os/hardware-before.json`. A limit set by a version
  before that record existed, or recorded outside 60, 80, and 100, is left
  as it is (the removal says so) until the firmware resets it.
- Restore checks the shared application catalog and writes nothing for an
  application that is gone; the person chooses another or clears the default.
  Two limits: an entry whose `TryExec` program is missing still counts as
  installed, and the catalog is read from the process's own `XDG_DATA_DIRS`, so
  a manager started without the Flatpak paths reports Flatpak applications as
  gone. The choice buttons in the window have not been looked at.
- The login screen watch (ticket 140, ADR 0033) has run against a real
  systemd, logind, and journal only in CI's container check
  (`packaging/boot-watch/run.sh`, a stand-in `gdm.service` that never opens
  a session); this machine has no Docker and no VM image. It has never
  watched a real boot or restarted a real GDM, and the greeter-seen path is
  proved by fakes only. It treats ticket 139's symptom; the cause is still
  ticket 139's.
- Which drives mount automatically (ticket 127, ADR 0032) lives only in the
  system's files: Better OS's marked block in `/etc/fstab` and
  `/etc/udev/rules.d/90-better-os-drives.rules`, written only by
  `manager-daemon`'s `org.betteros.Drives1` and read by both windows through
  `drive-mounts`, which also owns the drive listing. Never add a preference
  file for it, and never write an fstab line outside the block. No real drive
  has been mounted by it yet; the container check runs without systemd, so a
  mount there is reported as failed by design.
- Better Files reads device state from `org.betteros.Storage1` through
  `storage_service::StorageClient`, and runs the same `storage-core` state
  machine in its own process when that service is absent. The window says which
  it got, and the in-process case is drawn as a warning rather than a note,
  because an in-process engine never sees the tracked-operation notices another
  application would have sent to a service. Do not add a third fallback that
  invents a state.
- A Better Files job that writes to an external device is a tracked operation
  in the storage service from before its first write to after its last, and
  the service ends every operation a client started when that client leaves the
  bus. None of it has been exercised with a real USB device or a real crash
  mid-copy; the proof is a private-bus test and the in-process engine.
- Better Files does not offer Performance mode: `files_gui::policy::
  OFFER_PERFORMANCE_MODE` is off because the mode changes no mount option or
  cache setting and so makes no write faster. A device already in the mode is
  still offered Direct Removal. Turn it on only with the mount-option work
  ticket 31 left to an ADR, and reword the throughput risk text in the same
  change.
- Preview treats a parser as a boundary, not a sandbox. The size limit, the
  decoder's own allocation limits, and a `catch_unwind` are what exist;
  `engineering/specs/files-preview-policy.md` states what each one does and does not buy. A
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
- `app-chooser-core` writes `mimeapps.list` without guarding against another
  program changing it between the read and the rename. The rename also replaces
  a symbolic link with a regular file and replaces a file the person made
  read-only. Setting a default always worked this way, and removing one now
  takes the same path. Undoing a removal refuses when it cannot put the line
  back cleanly; no screen offers that undo yet.
- Better Files runs its operations as durable jobs now: `files-operations` owns
  copy, move, duplicate, rename, bulk rename, trash, restore, permanent delete,
  checksum, compress, and extract, and a job survives every handle to it being
  dropped. A job is a header plus an append-only item journal, a trash on a
  removable disk goes to that volume's own trash, and a copy keeps hard links.
  What is still open: the volume trash and hard links have been tested on
  temporary directories with a device-number seam, never on a second real
  device, and FAT or exFAT media not at all; the Trash view reads every mounted
  filesystem's trash, but a network or FUSE filesystem only through a probe
  with a one-second deadline, so such a trash can be missing from one listing.
- Extraction refuses absolute paths, `..`, and links that leave the target, and
  stops at the size and entry limits in `files-operations`' policy. Those limits
  were proved with small archives and lowered limits, never with a real
  decompression bomb. A zip's declared entry count and central-directory size
  are checked from its end record before the `zip` crate reads anything; when
  that record leads nowhere the crate searches for an earlier one and reads its
  directory unchecked, which only a parser of our own would close. Compression writes `.tar.zst` with
  the pure-Rust `ruzstd`, whose only level is its fastest, so those archives are
  larger than `zstd`'s default would make them. Zip reads only stored and
  deflate entries.
- A Better Files job lives as long as the login session and never resumes on
  its own (ADR 0016). Better Copy, a service that would run a job with no
  window open, is a future proposal that needs its own decision.
- Measure Better Files copy performance against real hardware before claiming
  it. Every published number is a page-cache number from an ext4 temporary
  directory: no spinning disk, no USB device with `fsync` per file, and no
  network share has been measured.
- The built-in catalog now carries all eight component manifests, so
  `better-manager list` offers the whole suite and `better-manager defaults
  inspect` reports Better Files' declared file-manager integration rather than
  an empty state. Better Files is the only component that
  declares a default integration; the other seven declare none, which is a
  deferred Issue #10 decision and not an omission this catalog change made.
- `packaging/prune-target.sh` frees build space: it lists what it would remove
  and removes it only with `--apply`. Run it on a worktree's own `target/` when
  the disk runs low, and never while cargo is building in that directory.
- The license inventory check catches a class of merge that is easy to make.
  `docs/third-party-licenses.md` pins the `Cargo.lock` hash, so any commit that
  moves the lockfile and does not regenerate it turns CI red on `main` — which
  is what the ticket-43 merge commit `c813fd6` did, over one added optional
  dependency. Regenerate in the same commit that moves the lockfile; a dependency
  change is a lockfile change even when it adds no new crate to the graph.
- One place reads os-release and decides which Ubuntu base a host is built
  on: `crates/better-core/src/host.rs`. `read_os_release_under` reads
  `/etc/os-release`, or `/usr/lib/os-release` only when the first does not
  exist, and every Rust reader calls it: `manager-daemon`, `manager-platform`'s
  `HostPlatform`, Better Manager's home banner, Better Monitor's audit, and
  the system drive's name (ticket 138, `running_system_name` through
  `drive_mounts::listing::Drive::naming`). `install.sh` — which is shell and
  cannot — keeps a matching copy of the file order and the release rules with
  a comment pointing at it and a CI fixture for every case.
  `crates/better-core/tests/os_release_readers.rs` fails when any other crate
  names an os-release path outside its tests; do not add a reader. A client
  that resolves the host differently from the service that checks its plan
  is the ticket 43 and ticket 45 defect in both of its forms. `MockPlatform`
  is the demo mode's platform and nothing else: a client that plans from it
  reports Ubuntu 24.04 amd64 whatever the machine is.
- `resolve_distribution_id` returns `/etc/os-release`'s own `ID`, so a Zorin
  host is `zorin` and not the Ubuntu base its release number comes from. That
  went public in v0.2.5 and it is a contract change for manifests, not only an
  internal one: `targets.distributions` is compared against the real ID, so a
  third-party manifest listing only `ubuntu` is refused on Zorin. All eight
  first-party manifests list both. The release axis is unaffected — 22.04 and
  24.04 still resolve through `UBUNTU_CODENAME` — and a file with no `ID` at all
  falls back to `ubuntu`, which is the only remaining thing to say about a host
  whose codename already named an Ubuntu release. Before adding a distribution
  to the matrix, decide whether the manifest field should keep naming exact IDs
  or start naming a base, because every derivative added under the current rule
  needs every manifest edited.
- A component's health check is dpkg's file list now, not a path derived from
  the package name. `/usr/bin/<component>` was the old rule and it was wrong
  about three of the eight shipped packages — `better-awake`, `better-storage`,
  and `better-manager-daemon` install no binary of their own name — so every
  install of them passed apt, failed the check, and was rolled back. An install,
  update, or restore is healthy when dpkg reports the package installed and
  every path `dpkg-query -L` lists for it exists. Three exemptions are
  deliberate and are the places to look first if the check ever passes something
  it should not: directories are skipped, conffiles are skipped because dpkg
  lets an owner delete one, and a package that lists no files at all is healthy
  because everything it listed is there. Do not reintroduce a path derived from
  a name, and do not read a path a manifest supplied — the file list is dpkg's
  record of the package the daemon itself verified and installed, which is why
  it may be trusted.
- **dpkg lists files it was configured never to install.** `--path-exclude`
  drops a file at unpack and leaves it in the package's file list, and
  `dpkg --verify` calls it `missing` as well, so no dpkg query can tell that
  absence from a broken package — only dpkg's configuration can.
  `better-core/src/package_health.rs` reads `/etc/dpkg/dpkg.cfg.d/*` and
  `/etc/dpkg/dpkg.cfg`, plus `apt-config dump DPkg::Options`, because the daemon
  installs through `apt-get`. The daemon's health check and the client's verify
  (`manager-platform`'s `verify_package`) both go through that module; do not
  write the rule a second time. This matters on every minimized image, the
  `ubuntu:24.04` container the end-to-end check runs in included: it excludes
  `/usr/share/doc/*`, and every Better OS package installs
  `THIRD-PARTY-LICENSES.md` there. A health check that ignored those filters
  would fail every package on such a machine. Two limits travel with it: a
  filter written into root's `~/.dpkg.cfg` is not read, because the driver
  clears the environment and never resolves a home directory, and the glob
  matcher is this project's own rather than the C library's `fnmatch`, so a
  pattern using anything beyond `*`, `?`, and `[...]` classes is not understood.
- The container end-to-end check now installs `better-awake` through the
  privileged service, for its shape rather than its features, and asserts that
  `/usr/bin/better-awake` does not exist. If that package ever gains a binary of
  its own name, the assertion fails on purpose: it is saying the test no longer
  proves what it was added to prove, and another multi-binary package has to
  take its place. The rollback fixture beside it is unhealthy because its
  postinst deletes a file dpkg has already recorded; a fixture that simply ships
  no files is healthy under the current rule and would silently stop testing
  rollback.
- Neither the container end-to-end check nor the footer has been observed since
  ticket 47, and no release since shipping them changed that. The e2e is CI-only
  from this worktree — no Docker daemon — and the footer's
  `Zorin OS 18 · Ubuntu 24.04 base` line is asserted by test from the real host
  probe's output rather than looked at on a running desktop. A published release
  is not a report from the machine that filed the defect: nobody has yet watched
  the Zorin 18 host install Better Awake on 0.2.6 or later and keep it, and
  nobody has watched it upgrade past 0.2.6 and find the update the drift had been
  hiding. Ticket 49's self-heal was proved against that machine's real state
  record copied to a scratch path, which is evidence about the record and not
  about the machine.
- **zbus stays on its default `async-io` flavor across this workspace.** The
  backend is a compile-time cargo feature and cargo unifies features across
  every package one `cargo build` names, so a single crate asking for
  `zbus/tokio` compiles that flavor into every binary built beside it —
  including every GPUI window, because `packaging/build-deb.sh` builds the
  services and the windows in one invocation. gpui's XDG desktop portal
  (`ashpd`) and `accesskit_unix` each open their own zbus connection from a
  plain thread no tokio runtime owns, and the tokio flavor panics there with
  "there is no reactor running". Neither thread is reachable from this project,
  so no runtime guard can cover them; ticket 43's guard in `privileged.rs` only
  ever covered our own calls, and is gone with the flavor that needed it.
  `crates/manager-platform/src/flavor.rs` fails the build with the file and line
  if the feature comes back. The cost is on the service side and is paid
  explicitly: zbus now polls a served method body on its own executor rather
  than as a tokio task, so a handler must not call `tokio::spawn` or
  `tokio::task::spawn_blocking` — the panic is swallowed inside the task and the
  *caller* hangs. `manager-daemon` and `storage-service` capture a
  `tokio::runtime::Handle` at construction and spawn through it. Any new handler
  that needs tokio must do the same.
- **`/usr/bin/better-manager` is the window; `/usr/bin/better-manager-cli` is
  the command line.** This is `better-monitor`'s split, and it is now the rule
  for a component that ships both. The window refuses an argument it does not
  understand with exit 2 and names the command line, rather than opening a
  window a person waiting at a terminal cannot escape. Both binaries are in the
  published `better-manager` package from v0.2.7 on. Every other window follows
  the same rule through `better_ui::command_line`: it answers `--help` and
  `--version`, keeps the flags its desktop entries pass, and refuses the rest
  with exit 2. Better Files also takes the location the desktop hands it.
- Better Launcher takes its grid navigation keys as the search field's own
  cursor actions (`capture_action` on the overlay root), because gpui dispatches
  a keymap action before any key listener and gpui-component's single-line
  input handles left, right, Home, and End itself. If gpui-component rebinds
  one of those keys, the overlay's dispatch test is what catches it.
- Reconciliation adopts a host that is **ahead** of the record and blocks on
  every other disagreement. An external `apt` or `install.sh` upgrade is
  supported — it is the only way the manager can be upgraded — so a newer
  version under dpkg's ownership is the machine having moved forward, not drift,
  and it clears the failure and failed health that describe the version it
  replaced. A host that is behind, a package the record claims and dpkg does not
  have, and a version `semver` cannot order all stay blocking findings, and the
  way out of one is `Manager::adopt_host_state` — from the component page or
  from `better-manager-cli reconcile --adopt <id>`, never by editing the state
  file. Both shipped in v0.2.7. Adoption drops the restore snapshot and the recorded artifact, because
  they name a version the machine no longer has; the GUI says so before it acts.
- A package dpkg holds at a version `semver` cannot order (a tilde version
  such as `0.2.8~rc1`) is never adopted and never planned for. Doctor lists it
  with dpkg's own version string, and the component page explains it in place
  of an install button. `better-manager-cli plan` is a simulation and still
  prints a plan for it; `run` refuses.
