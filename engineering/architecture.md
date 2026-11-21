# Architecture: Better OS

The system as it is now. Why each hard-to-reverse choice was made is in the decision records listed at the end.

## Components and boundaries

```text
component manifests ──> better-core validation ──> manager-core
                                                   plan + mock lifecycle state
                       manager-platform ──────────────┘  ├── manager-cli ──┐
                       capability/download/package       └── manager-gui ──┼── manager-store JSON
                       manifest fetcher (catalog)                          └── versioned local state
                       privileged executor (refuses)                          + catalog cache

published manifests on main ──> manager-platform::catalog_fetch (HTTPS, bounded)
                                └──> manager_core::catalog  validate, guard, degrade
                                     └──> manager-store  versioned cache, state dir

monitor-collectors-linux (/proc, /sys) ──> monitor-core metric contracts
                        └──> monitor-service (owns collection, store, audits)
                             monitor-store / monitor-ipc / monitor-export
                             └──> monitor-gui, monitor-cli (both clients)

better-ui provides GPUI presentation primitives to both GUI crates.
Privileged execution lives in `manager-daemon`, a separate root process reached
over the D-Bus system bus and authorized by polkit. See ADR 0007.

Release builds package the final GUI binaries as `.deb` assets. Package
metadata declares runtime dependencies; the development-only GPUI linker
packages stay in the build environment and are never a user installation
prerequisite.
```

### Component suite

Tickets 18 through 35 expand the workspace from Better Manager alone to the
full component suite. Two crates are shared infrastructure that several
components read, and neither may be reimplemented by a consumer.

```text
app-catalog-core ──> app-catalog-platform      one desktop-entry scanner, shared
   ApplicationRecord   XDG discovery, watching, launching
        │
        ├──> app-chooser-core ──> app-chooser-gui      MIME ranking, AppSelection
        ├──> launcher-core ──────> launcher-platform ──> launcher-gui
        │       index, matching, deterministic ranking
        ├──> files-core ─────────> files-gui           Applications location
        └──> defaults-platform                          restore checks what is installed

storage-core ──> storage-platform ──> storage-service ──> files-gui
   device identity, Direct Removal policy, state machine, typed device states

files-core ──> files-platform ──> files-operations ──> files-gui
   typed locations   fs/XDG/MIME/trash   durable job engine   window, views
files-preview ──┬──> files-gui          bounded, cancellable, parser as boundary
files-search  ──┘                        provider / ranking / UI, kept apart

monitor-core ──> monitor-collectors-linux ──> monitor-service ──> monitor-gui
   typed metric/capability contracts     monitor-ipc / monitor-store / monitor-cli
   typed process actions ──> monitor-actions-linux   the only caller of kill(2)
        └──> monitor-views ──────────────────────────> monitor-gui
             grouping evidence, table/apps/overview models, no GPUI

awake-core ──> awake-platform ──> awake-service ──> awake-ipc ──┬── awake-tray
   sessions, policies, trigger rules   owns inhibitors          └── awake-gui
                                       awake-store

touchpad-core ──> touchpad-platform ──> touchpad-gui
   versioned config     device/session adapters
touchpad-gestures ──> touchpad-session ──> better-actions ──> launcher-gui
   gesture model, preset, conflicts    typed desktop actions
        └──> touchpad-gesture-service ──> better-touchpad-gestured
             resident pipeline: compositor events in, typed actions out
adapters/gnome-shell-touchpad/   GJS under ADR 0012's bounded exception
touchpad-scroll ──> better-touchpad-scrolld (one system service per touchpad)
   device rule, frame scaling, grab lifetime, evdev/uinput layer (ADR 0030)
        └──> touchpad-platform::scroll   the window's system-bus client

window-attention-core ──> window-attention-service ──> better-window-attention
   choices, decision, store   resident relay counterpart (ships in better-manager)
        └──> manager-gui                               Window attention page
adapters/gnome-shell-adapter/    shared relay-only GJS extension (ADR 0018)

hardware-core ──> hardware-platform ──┬──> manager-gui        System page (ticket 86)
   what a machine has, each card's     ├──> manager-daemon     org.betteros.Hardware1
   rules, no I/O                       └──> better-window-attention  charger switch
   sysfs under a root, PPD / UPower / gsd-power clients, the Hardware1 client
hardware-core::charge_state   what every battery icon shows (ticket 170): one
   rule, its pictures' names and its words; also "is a charger online"
   ├──> monitor-collectors-linux   the kernel's battery status, parsed once
   ├──> monitor-gui, monitor-tray, manager-gui   every battery icon and line
   ├──> awake-platform              which power supplies count as a charger
   └──> awake-gui ──> hardware-platform   reads the battery beside the service

drive-mounts ──┬──> files-gui         sidebar, "Mount automatically" (ticket 127)
   which drives  ├──> manager-gui       System page, Drives section
   the listing,  └──> manager-daemon    org.betteros.Drives1
   fstab block, udev rules, choices, the Drives1 and systemd clients

defaults-core ──> defaults-platform ──> defaults-store ──> manager-gui / CLI
   declarations, aggregate status, plans   typed adapters   snapshots + history

patch-core ──> patch-platform ──> manager-gui        Fixes page (ticket 90)
   known-fault entries: detect,   Flatpak files, Mutter's scale, the closed
   describe, plan, check, undo;   command list, the record of applied fixes,
   no I/O                         a fixture machine for the demo and sandboxes
```

`better-ui` provides GPUI presentation primitives to every GUI crate in the
suite, not only the two that exist today.

GPUI comes from the owner's fork of Zed, `https://github.com/TimLai666/zed`,
branch `better-os/gpui-<pinned revision>`: the pinned upstream revision plus
only fixes also offered upstream, one commit each. The workspace still names
`https://github.com/zed-industries/zed` in its dependencies and redirects
every crate from it, `gpui-component`'s own included, with one
`[patch."https://github.com/zed-industries/zed"]` section at an exact
revision ([ADR 0043](/engineering/decisions/0043-gpui-from-a-fork.md)).

### The Better OS session (being built)

The owner chose a Better OS desktop session on COSMIC's compositor beside
GNOME ([ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md)); what
it promises is `engineering/specs/better-os-session.md`, and tickets 200 to 229
build it. GNOME stays the default until the spec's bar is met. Nothing below
exists yet; it is the structure the tickets build to.

```text
GDM ──> better-os.desktop ──> session-launcher (start) ──> cosmic-comp ──> session-launcher --ready
                                 environment, dconf profile    upstream,      imports env, starts
                                 betteros, portal config       GPL-3.0-only   better-os-session.target
                                                                 │
                     Wayland: layer-shell, toplevel info/manager, workspaces, image copy capture,
                     input method v2; later betteros-gesture-unstable-v1 (Better OS's patch)
                                                                 │
   COSMIC daemons (System76's pinned packages)        Better OS surfaces (GPUI on better_ui::layer)
   cosmic-settings-daemon  cosmic-osd (polkit agent)  top bar + top-bar-xembed   taskbar
   cosmic-idle  cosmic-bg  the cosmic-greeter locker  Better Launcher panel      window switcher
   xdg-desktop-portal-cosmic  COSMIC Settings         overview                   notification service,
   Ubuntu: Fcitx 5 + Chewing, Tracker, gnome-keyring   Better Files desktop       island, center
                                                                 │
   every existing Better OS user unit (graphical-session.target, which the session target binds)
   touchpad-gesture-service ── claims swipes and pinches from the patched compositor ──>
        org.betteros.TouchpadGestures1 Transition* signals ──> the surfaces draw the transition

Better Manager ──> manager-core ──> AccountsService SetSession (the person's own account)
            └────> manager-daemon: System76's source, key, pin, and the 14-package closure
                   from a closed table compiled into the daemon; deferred purge after logout
```

- **Upstream as released, Better OS on top.** The compositor and the COSMIC
  daemons are System76's packages taken as released (C8: unmodified first);
  every surface is a Better OS Rust process on layer-shell; the session uses no
  GJS. `cosmic-session` is not used, because its component list is fixed in
  code; `session-launcher` and the systemd target replace it.
- **One unit, two sessions.** Existing user units stay `WantedBy=graphical-session.target`;
  the session target `BindsTo=` it. A component's desktop-specific half picks its
  backend at run time from `better_core::desktop`, the one place the session is
  read; the session's own surfaces are `WantedBy=better-os-session.target` and
  never start under GNOME.
- **The session never writes GNOME's settings.** Its dconf profile writes to
  its own database (`user-db:betteros`, then GNOME's `user` read through), its
  COSMIC configuration is its own, and carrying settings over is one way, from
  GNOME into the session. `defaults-platform`'s dconf writer writes the first
  database of the profile the target session uses (ticket 207); a cosmic-config
  adapter beside it does preview, capture, apply, verify, and restore for COSMIC
  keys (ticket 206).
- **A third-party archive through the privileged service.** Until now
  `manager-daemon` installed only local artifacts it verified by a manifest's
  checksum. For the session it also installs a closed list of packages from
  one archive APT verifies by signature (ADR 0038 C2): the source URL, the key
  fingerprint and bytes (shipped in `better-manager-daemon`), and the 14 names
  are compiled into the daemon; no plan or manifest can name another source,
  key, or package; the pin gives that origin priority −1 except those names; a
  pin or source file that differs from the table is a finding, never
  overwritten. This is the only exception C2 made to AGENTS.md's rule on APT
  repositories.
- **The compositor patch is a kept fork of one change**
  ([ADR 0039](/engineering/decisions/0039-compositor-fork-policy.md), proposed).
  The protocol XML is Better OS's own (`GPL-3.0-or-later`); the patch series
  and the patched `cosmic-comp` are `GPL-3.0-only`, as the work they modify,
  and sit together with the rest of this `GPL-3.0-or-later` project because
  "or later" includes version 3 and Better OS's processes reach the compositor
  only over Wayland and D-Bus. The recommended layout keeps the series in this
  repository under `compositor/cosmic-comp/`, pinned to an Epoch release tag,
  built in ADR 0021's environments, with the exact upstream tarball, the
  series, and the build script published beside the `.deb` as its
  Corresponding Source. Each Epoch is followed by a closure check, a rebase,
  the protocol tests, and a VM flow before release (ticket 226); the series is
  dropped when upstream hands gesture progress to an outside client.
- **The gesture contract** keeps decisions in Rust outside the compositor:
  the patch reports what libinput reported to the one claiming client (checked
  by executable) and drives the compositor's own workspace animation when
  asked; thresholds, directions, and actions stay in `touchpad-gestures`; the
  surfaces only draw the progress they are sent, accepting it only from the
  gesture service's executable (ADR 0035's rule). The contract is in the spec.

### Component seams

- **The catalog is fetched, validated, and allowed to be stale out loud.** A
  manifest checksum can only exist after its release does, so a compiled-in
  catalog always describes the previous release.
  `manager-platform::catalog_fetch` fetches bytes over HTTPS and interprets
  none of them; `manager_core::catalog` applies the same `better-core`
  validation a built-in manifest gets, plus a file-name-to-component-ID check, a
  downgrade guard, and a whole-set assembly check; `manager-store` keeps the
  versioned cache in the state directory and re-validates it on read, so a
  tampered cache is an absence rather than a catalog. There is exactly one
  definition of the built-in seven, and both the command line and the window
  start from it. Four degraded states are representable and every one of them is
  on screen — there is no state meaning "probably current". See
  [ADR 0013](/engineering/decisions/0013-remote-catalog-refresh.md).

- **One catalog, no second scanner.** `app-catalog-core` and
  `app-catalog-platform` are the only place a `.desktop` file is parsed. The
  chooser, the launcher index, the Applications location, and Better Defaults'
  restore consume records.
  Desktop entries are untrusted input and are validated before any consumer sees
  them, the same way `better-core` treats a component manifest.
- **Launching never builds a shell string.** Every component that starts an
  application goes through the shared platform launch path, which uses the
  registered desktop definition, honors D-Bus activation, and validates targets.
  Executable resolution is a reported status; a Flatpak, Snap, wrapper, or
  D-Bus-activated entry has no fabricated executable path.
- **A location is a type, not a path.** `files-core` carries a typed location and
  URI abstraction. Trash, Recent, Applications, external devices, and future
  network locations are representable without any of them being a special-cased
  `PathBuf`.
- **Jobs outlive windows.** `files-operations` owns copy, move, trash, restore,
  and delete as durable jobs with persisted state. A closed or crashed window
  never leaves a job in an unknowable state.
- **Collection outlives the GUI.** `monitor-service` owns historical collection,
  and `monitor-gui` reads through `monitor-ipc`. The GUI does not scrape `/proc`,
  run a shell pipeline, or hold a privileged handle. With no service running the
  window starts the same engine in its own process and says so on every stored-
  data page, because history it is not recording will not be there later.
- **A gap is a record, not an absence.** `monitor-store` writes a gap whenever
  the service stopped, fell behind, recovered a torn write, or dropped history
  to stay inside retention. Charts leave a space where a reading is missing and
  draw a floor where one measured zero; the two are never the same pixel.
- **Redaction is a boundary, not a filter.** `monitor-export` is the only path
  data leaves the machine and has no network code. Command-line arguments are
  dropped whole rather than scanned, because arguments are where credentials
  live and a scanner that is right nine times in ten is worse than useless
  there.
- **A view decision is not a rendering decision.** `monitor-views` owns app
  grouping, sorting, filtering, tree building, aggregation, and the Overview
  verdicts, and has no GPUI dependency, so all of it is tested without a
  display server. `monitor-gui` draws models; it never decides what a number
  means.
- **A group is a claim that carries its evidence.** Processes are only merged
  when the system recorded a relationship — unit membership, Flatpak or Snap
  identity, a desktop launch, or a parent link. Matching executable names never
  merge anything; the fallback key carries the PID. The precedence is
  configuration with a recorded default, not a decided answer.
- **An intent, not a signal.** `monitor-core::action` carries what the user
  asked for and a closed set of signal kinds; `monitor-actions-linux` is the
  only crate that maps one to a number and calls `kill(2)`. Refusals are data
  the button is rendered from, decided before anything is attempted, by one
  policy function both the real controller and the test fake apply.
- **A preview is a refusal that carries its reason.** `files-preview` applies
  the size limit before a provider is called, hands the decoder its own
  allocation limits, and catches a panicking parser. Every refusal produces a
  metadata preview with a stable reason key; there is no path that returns
  nothing and no path that draws an empty pane. This is a boundary, not a
  sandbox, and `engineering/specs/files-preview-policy.md` says which is which.
- **Search separates provider, ranking, and UI before it needs to.**
  `files-search` ships one provider — the current location, fed the entries the
  pane already holds, so searching where you are costs no I/O. `RunDemand` exists
  so an indexed provider can produce its own candidates instead, which is the
  seam Issue #6 asks for while the indexed engine is still an open decision.
  Ordering is decided rather than emergent: match kind dominates, ties break on
  length and then natural order, and there is no tunable weight anywhere.
- **Five states, not one number.** Monitor metric contracts distinguish unknown,
  unsupported, permission denied, stale, and zero. Storage distinguishes Ready to
  unplug, Writing, Busy, Performance, and Unknown. Defaults distinguishes eight
  aggregate states. In all three, an unverifiable condition is reported, never
  rendered as a reassuring value.
- **The service owns the token.** `awake-service` holds the inhibitor;
  `awake-tray` and `awake-gui` are clients that can restart without ending a
  session. The tray executes no shell command.
- **A rule is data, and unknown is not false.** An awake `Condition` is a closed
  enum with validated operands, so no shell command has a shape it could take.
  Evaluation is three-valued: a provider that cannot be read yields unknown,
  which never becomes true — so an unreadable provider never keeps a machine
  awake — and never becomes false, so the editor explains the control instead of
  rendering an inert one. Priority names the winner of a disagreement; it never
  weakens a protection, and the battery threshold that stops first wins whatever
  the priorities say. See [ADR 0010](/engineering/decisions/0010-awake-trigger-providers-and-retention.md).
- **The desktop is changed through typed adapters.** `defaults-platform` and
  `touchpad-platform` expose read, apply, and verify as traits returning typed
  values. GPUI code never invokes `gsettings`, `xdg-mime`, or a shell command,
  and an integration kind with no production adapter reports manual action
  required rather than guessing.
- **Capture before the first mutation.** Defaults snapshots, touchpad backups,
  and MIME association rollback records are all written before the first change
  to a setting, and restore returns to the captured value rather than a guessed
  factory default. Reading the effective state again before applying is how an
  external change is detected instead of overwritten. A changed-externally entry
  is held back until that exact entry is confirmed; confirming one confirms
  nothing else. See [ADR 0009](/engineering/decisions/0009-defaults-declarations-and-adapters.md).
- **Gesture logic stays in Rust core crates.** `touchpad-gestures` owns the
  model, preset, and conflict detection; `touchpad-session` is a narrow adapter
  that invokes typed `better-actions` and never accepts an arbitrary shell
  string from configuration. The launcher's gesture seam is the same boundary
  seen from the other side, and the two speak one vocabulary: Better Touchpad's
  event kinds map onto `launcher-platform`'s four phases with nothing lost, and
  a test drives the launcher's own recognizer from a Better Touchpad stream.
- **Arbitrary execution is unrepresentable, not merely forbidden.**
  `better-actions` is a closed enum with no command, path, or free-text
  variant. Its one piece of user text is a keyboard shortcut whose key comes
  from a fixed table behind a private field, so a stored file, a D-Bus message,
  and a GUI field all have nowhere to put a shell string.
- **A recognizer is business logic, not integration.**
  `touchpad-gestures::recognizer` takes contact-point frames and emits gesture
  events, holds no clock, and knows about no device. Activation, cancellation,
  cooldown, thumb detection, and every preset gesture are therefore tested by
  replaying a stream, and whichever backend ADR 0012 delivers plugs in front of
  it without moving a decision into it.
- **A compositor's events become frames, not a second recognizer.**
  `touchpad-gestures::ingest` turns a GNOME Shell gesture event into the contact
  frame it would have been and feeds the same recognizer. There is one
  activation threshold, one cancellation rule, and one cooldown in this project,
  and a backend that reports gestures rather than contacts does not get its own
  copy of them.
- **The pipeline outlives the window.** `touchpad-gesture-service` is the
  resident half — `better-touchpad-gestured` — because a gesture that only
  worked while a settings window was open would be a gesture nobody could rely
  on. It sits above `touchpad-gestures` and `touchpad-session` because it needs
  both, links no toolkit, and holds no privilege. The window and the service
  share one file, one auto-disable rule, and one suppression state machine
  rather than a copy each.
- **The GJS adapter is a bridge with a test around its bounds.**
  `adapters/gnome-shell-touchpad/` reports what the compositor saw and performs
  what GNOME Shell already exposes. `touchpad-session::gnome` is the Rust half,
  written against a `ShellBridge` seam so the whole adapter is testable with no
  bus, and `crates/touchpad-session/tests/extension.rs` asserts the JavaScript
  against the bounds ADR 0012 set rather than trusting a review to notice.
- **The gesture half keeps its own state.** `gestures.json` and
  `gestures-backup.json` sit beside the pointer, scrolling, and clicking files
  and are written through `touchpad-core`'s own atomic-write and write-once
  machinery. A failing gesture adapter, or a gesture configuration that will
  not parse, cannot reach pointer movement or two-finger scrolling — which is
  asserted, not reasoned about.
- **The scroll factors are the one input path Better OS sits in.**
  `touchpad-scroll` grabs a touchpad's evdev node and re-emits it through a
  uinput clone with two-finger and wheel motion scaled (ADR 0030). It is the
  only first-party code that opens `/dev/input` or `/dev/uinput`, it runs as a
  udev-started system service per touchpad that drops privilege after opening
  its two nodes, and every decision (which device, how a frame is scaled, when
  the grab is taken and released) is in the library behind a device seam. The
  security review is `engineering/specs/touchpad-input-passthrough-security.md`.

### Trust boundaries

Manifest files, release metadata, collector output, and future remote data are
untrusted. Parsing and validation happen before planning. A manifest also
declares its own presentation and restart metadata, which is validated and
restricted to a closed icon set before any screen uses it. The GUI never
performs a privileged change itself; it asks the privileged service, which
authorizes the caller through polkit and revalidates the plan against the host
before acting. The service accepts an explicit, validated transaction plan and
returns an execution log, health result, and rollback record. An executor
without an authorized connection to it refuses every request.

## Data flow

1. The CLI or GUI loads manifests from the catalog.
2. `better-core` parses and validates each manifest.
3. `manager-store` loads a versioned JSON state file. A future schema is
   preserved, malformed current-schema input is backed up, and stale writers
   are rejected.
4. `manager-platform` reports the host profile, fetches artifacts, and reads
   installed versions from dpkg. Reading the host is unprivileged; changing it
   is not, and only an executor holding an authorized connection to the
   privileged service can do it.
5. `manager-core` validates the state against the catalog, resolves declared
   download, release-note, and disk-space metadata, produces a deterministic
   dry-run plan, and owns every mock lifecycle transition. A plan carries the
   declared replacements, enhancements, and restart scope, and a transaction
   inherits the widest interruption its steps declare. A plan is rejected when
   declared disk space exceeds the reported profile. No plan step can mutate
   the host in this stage. Its errors expose stable machine keys; presentation
   layers own localized user-facing wording.
6. The CLI and GUI both review the same plan, persist each mock stage, and can
   resume a valid active operation after restart.
7. `monitor-collectors-linux` reads `/proc` and `/sys` directly and emits
   `monitor-core` reports. A metric identity carries its unit, semantic type,
   source, and sampling behavior, and an observation keeps unknown,
   unsupported, permission-denied, stale, and a measured zero apart. Every
   read goes through a root path, so tests drive the production path against
   captured `/proc` snapshots. Storage remains in-memory until the time-series
   decision has an ADR.
8. Release packaging derives and verifies runtime dependencies from the final
   binaries for each supported target and architecture.
9. A clean desktop environment installs the `.deb` through local APT and runs
   the manager and monitor launch smoke tests.

### Manager transaction shape

1. Resolve a component and its manifest dependencies.
2. Verify target compatibility and artifact metadata against the profile
   `manager-platform` reports.
3. Resolve declared artifact download and disk requirements, reject an
   insufficient mock profile, and produce a dry-run plan that requires a
   current state revision at approval. Missing catalog or profile data stays
   explicitly unavailable rather than becoming a guessed estimate.
4. Advance the approved plan through download, install, settings, and health
   stages, persisting each state as it goes. A driver decides what each stage
   actually did: a simulation scripts it, a real transaction observes it.
5. Record failure evidence and the restore outcome when verification fails.
   Carry the declared replacements, enhancements, and the widest restart
   requirement into the plan so a reviewer sees them before approving.
6. For a real transaction, fetch and verify every artifact, hand each to the
   privileged service over a file descriptor, and give it the whole plan. The
   service revalidates independently, applies through local APT, health-checks
   what it applied, and rolls back what it can. The executor was replaced
   without changing the GUI or CLI contract: the same lifecycle stages, driven
   by a different driver.

Lifecycle descriptors remain data and are never interpreted as commands, on
either side of the privileged boundary.

### Monitor observation layers

- Continuous: low-cost CPU, memory, PSI, process, storage, and network
  collection.
- Periodic: audits that refresh component and system state.
- Event-triggered: a slowdown the service recognizes by itself (sustained
  pressure, memory nearly gone while swapping, an out-of-memory kill, thermal
  throttling, or the sampling loop itself freezing) is recorded as an
  incident with the state around it (ticket 115). Nobody marks one by hand.

Collectors read `/proc` and `/sys` directly. They never run a command or parse
a tool's human-formatted output, because the kernel already publishes a stable
structured interface for everything they collect. Every read goes through a
root-path parameter, so tests exercise the production path against captured
`/proc` snapshots.

A reading is only meaningful next to its metric identity, which names the unit,
the semantic type, the source file, and how the value has to be sampled. The
absence of a reading is data too: unknown, unsupported, permission denied,
stale, and a measured zero are five distinct states, so an unobserved subsystem
can never be presented as an idle one.

The monitor stores locally by default. Export is explicit, keeps the
observation state of every reading, and redacts sensitive values before data
leaves the local process. Process command lines are not collected unless
collection is explicitly configured to include them.

## Test seams

- `test-prerequisites` is the one rule for a test that needs something the
  machine may not have (a program such as `dbus-daemon`, `dconf-service`, or
  `gjs`, a locale, a filesystem feature, a user other than root): a skip on a
  developer's machine and a failure under CI (ticket 177). It is a crate with
  no dependencies at all, so every crate's tests take it as a dev-dependency
  without a cycle or a heavy edge, `better-core`'s included, and
  `defaults-platform`'s `test-support` feature uses it for the private dconf
  service. Its own test fails when a crate prints a skip of its own or reads
  `CI` itself, and CI fails a test log that contains a skip line.
- Manifest behavior is tested through public parse and catalog-validation APIs.
- Manager behavior uses mock state and asserts observable plans, stages,
  failure evidence, recovery results, and status results.
- `manager-store` is an adapter seam for versioned JSON reload, stale-write,
  corrupt-state, and restart-resume tests. It never decides a lifecycle state.
- `manager-platform` is the host seam. Its tests assert that the mock reports
  the profile it was given, that an unavailable free-disk value stays
  unavailable, and that no backend applies a change without an authorized
  privileged connection. That last assertion replaced an earlier one that no
  shipped backend could apply a change at all; real installation exists now, so
  the invariant had to say something narrower and still true rather than be
  deleted.
- `manager-daemon` is the privileged seam. Its APT driver, host probe, health
  probe, and authorizer are all traits, so every transaction path — including
  each rollback outcome — is tested without privileges, plus a private
  session-bus test of the D-Bus surface itself.
- Monitor contract behavior uses in-memory history and asserts that the five
  observation states stay distinct. Process-action policy is tested through a
  recording controller that shares the production availability rule, so a test
  that proves a refusal proves the rule production applies.
- `monitor-views` is tested twice over: by hand-built facts for every ordering,
  filtering, aggregation, and verdict rule, and by running the production
  collectors over ticket 22's captured `/proc` trees, so a renamed metric fails
  a test instead of blanking a column.
- `patch-core` entries are tested through a fake host of canned answers that
  follows each change; `patch-platform` runs every entry's detection,
  fix, check, and undo against a fixture machine in a scratch directory with
  the real file reading and writing, and asserts over both crates' source
  that nothing names a shell and only its `command` module starts a process.
  No test runs `fc-cache` or `flatpak` or reads the person's files.
- `monitor-actions-linux` is tested against a real child process it spawns —
  stopped, resumed, reniced, terminated, and checked against `/proc` after each
  step — because a policy test cannot prove a syscall works.
- `monitor-gui` asserts what can be asserted without a window: both locales
  complete and actually different, every column header fitting its column in
  both languages, the action row's reflow at 100/125/150%, and the rule that no
  column renders a missing reading as a number. Collector behavior is tested against
  captured `/proc` and `/sys` fixture trees through the `Roots` seam, including
  truncated and malformed input, two-sample deltas, a kernel without
  `CONFIG_PSI`, and an unreadable descriptor directory. Collection cost is
  measured rather than asserted from a budget.
- `manager-gui` uses the demo catalog and state to assert that its Update All
  path produces the same dry-run plan as `manager-core`, that every catalog
  component is presentable from its manifest with no hardcoded component IDs,
  that a pre-theme state file loads dark, and both locales plus 100/125/150%
  long-label layout policy. Its Defaults screens keep every decision in
  `defaults_model`, which has no GPUI dependency, so the eight aggregate states,
  the review selection, the bottom summary, and the per-entry results are all
  asserted without a window. Two structural rules are asserted rather than
  reviewed: a plan is executed from exactly one place, behind a type only a
  review screen can produce, and the crate's shipped dependency list does not
  name `defaults-platform`.
- GUI crates depend on the shared core crates; launch smoke coverage runs in an
  environment with a display backend.
- `app-catalog-core` is tested through recorded desktop-entry fixture trees, so
  every visibility, normalization, and rejection rule is asserted without the
  host's installed applications. Its benchmarks use a 5,000-record synthetic
  catalog rather than whatever happens to be installed.
- `launcher-core` is the ranking seam. Its determinism and latency tests run
  with no display backend at all, which is the point of keeping GPUI out of it.
- Monitor collectors are tested against recorded `/proc` and `/sys` fixture
  trees. A live host proves nothing repeatable about a parser.
- `storage-core` is tested by replaying recorded UDisks2 and kernel event
  sequences through the state machine, including every failure case, because
  unplugging a real disk mid-write is not a test that can run in CI.
- `awake-service` is tested through a fake inhibitor backend for acquire,
  verify, lost-inhibitor, and release, plus a private session-bus test of the
  StatusNotifierItem registration — the same shape as the `manager-daemon`
  D-Bus tests. Its rule and battery paths run the production providers against a
  captured `/proc` and `/sys` tree, so a low-battery stop is driven by writing a
  file rather than by pushing a percentage in.
- `awake-platform` reads every kernel interface through a `Roots` seam, the same
  one `monitor-collectors-linux` uses. It deliberately does not import that
  crate: its read helpers return `monitor_core::Observation`, so reusing them
  would put the whole Better Monitor metric-contract stack in every Better Awake
  binary to answer whether the charger is plugged in.
- `hardware-platform` reads hwmon through `monitor-collectors-linux`'s
  `scan_hwmon`, behind its `sensors` feature, so the suite has one reader of
  `class/hwmon`. That puts `monitor-core` in Better Manager's window, which
  `awake-platform` avoided for Better Awake; the window already carries far
  more, and two hwmon readers would disagree about a chip sooner or later. The
  daemon asks for no features: it only finds two attributes, and their
  discovery (`sysfs::charge_limit_attribute`, `sysfs::fan_mode_attribute`) is
  the same code the window reads with.
- The two root writes of the System page go through `manager-daemon`'s
  `org.betteros.Hardware1` (ADR 0017), one polkit action per method, the
  argument checked against its closed set before polkit is asked. The daemon
  cannot write `/sys` itself (`BindReadOnlyPaths=/sys`); each write is a
  transient helper unit that sees only the one attribute file (ADR 0029). The
  keyboard's colour (ticket 136, ADR 0034) is the third: `KeyboardLighting`
  and `SetKeyboardLighting` on the same interface, for the built-in keyboard
  only. A HID LampArray is driven by a helper unit whose private `/dev` holds
  its one hidraw node and which drops to `nobody` once the node is open,
  running `--lamp-array`; both helpers share `hardware::helper_sandbox()`; the protocol (descriptor parser, report layout, scaling) is
  `hardware_core::lamparray`, with no I/O, and the hidraw ioctls are in
  `manager_daemon::keyboard_lighting` alone. asus-wmi's `kbd_rgb_mode` goes
  through the attribute helper. No session process opens a hidraw node.
  Everything else on the page needs no root and goes to the desktop's own services:
  power-profiles-daemon, gsd-power, UPower. "Switch power mode with the
  charger" runs in `better-window-attention`, the session service the
  `better-manager` package already ships, reading the person's choice from
  `$XDG_CONFIG_HOME/better-os/power-follow.json` at each plug event.
- Which drives mount automatically (ticket 127, ADR 0032) is kept in the
  system's own files, read unprivileged by both windows through
  `drive-mounts` and written only by `manager-daemon`'s
  `org.betteros.Drives1`, behind `org.betteros.manager.drive-mounting`. An
  internal drive is a line in Better OS's block in `/etc/fstab`, started and
  stopped as systemd's generated mount unit; a removable drive turned off is
  a `UDISKS_AUTO=0` line in `/etc/udev/rules.d/90-better-os-drives.rules`.
  The only argument is a UUID; the daemon asks UDisks2 what the drive is. A
  removable drive is mounted and unmounted by the window through UDisks2, as
  the person may. The daemon's prerm on removal runs
  `--remove-drive-entries`.
- Better Patch (ticket 90, ADR 0017, `engineering/specs/better-patch.md`) is
  the Fixes page. `patch-core` holds every entry as typed Rust: what it reads,
  how it decides, the exact change as data, the check, and the undo planned
  from the record of what the fix changed. `patch-platform` answers the reads
  and makes the changes: it edits the person's own Flatpak override file only
  while it still holds what was read, asks Mutter's `DisplayConfig` for the
  scale, and runs `fc-match`, `fc-cache -f`, and `flatpak override --show`
  from a closed list with fixed arguments, in one module. The two USB-C
  entries (ticket 101, ADR 0046) are the first that need root: their changes
  are `Change::Privileged`, typed methods of `manager-daemon`'s
  `org.betteros.UsbRecovery1` (`ReloadUcsi`, `RebindController`,
  `SetChoice`), each behind its own polkit action, and their fix is kept by
  the machine (`/var/lib/better-os/usb-recovery.json`), not the person's
  record. The kernel log is read as the person with `journalctl` when their
  account may read the system journal, and otherwise through the daemon's
  `Findings`, which returns only `hardware_core::usb_recovery`'s classified
  findings. The reload of `ucsi_acpi` and the rebind of one xHCI function
  each run in a transient helper unit built on `hardware::helper_sandbox()`;
  the package's `systemd-sleep` hook only calls the daemon's program, which
  queues one after-sleep unit when the person turned a fix on.
- The login screen watch (ticket 140, ADR 0033) is the daemon binary's
  `--watch-login-screen` mode, run once per boot by
  `better-greeter-watch.service`, which nothing in the boot waits for. It
  reads logind and the journal, restarts `gdm.service` at most once per boot
  and never while a person's session or an automatic login exists, and
  writes under `/var/lib/better-os/boot-watch/`: a 0644 summary anyone
  reads, and the evidence, journal excerpts, 0640 root:adm in `evidence/`. `manager_ipc::boot_watch` owns the
  records' format, `manager_core::login_watch` what a person is told (Home,
  Health, `better-manager-cli doctor` read the files unprivileged), and the
  off switch is `org.betteros.BootWatch1.SetEnabled` behind
  `org.betteros.manager.login-screen-watch`.
- `touchpad-platform` writes and reads what it claims. Its GVariant change-set
  encoder, which lives in `defaults-platform` and is shared with Better
  Defaults, is pinned byte-for-byte against values GLib produced, including the
  boundary where framing offsets widen, because a hand-rolled encoder that
  drifts from the specification is accepted by the dconf service and writes
  nothing. Device parsing runs over recorded `/proc` and `/sys` trees through a
  `Roots` seam. Every apply outcome — applied, awaiting sign-out, partially
  supported, failed, unsupported — is reached through the production
  write-then-read-back code with only the write and the read replaced. Exactly
  one test changes a real setting; it is `#[ignore]`d, needs an environment
  variable as well, and puts the setting back.
- `touchpad-gui` keeps every decision in `model.rs`, which has no GPUI
  dependency, so the control set, the unavailable explanations, the four apply
  states, the restore review, and both locales at 100/125/150% are all asserted
  without a window. A source-level test asserts the crate names no backend
  command.
- `defaults-platform` ships a mock adapter for every declared integration kind,
  so aggregate status, partial failure, and external-change detection are all
  provable before a real adapter exists for that kind. Its two real adapters are
  tested against recorded input rather than the running desktop: `mimeapps.list`
  content for the XDG adapter, and a `dconf compile` fixture database for the
  GNOME one. The GNOME adapter's writes go through a `ChangesetSender` seam: a
  fake stands in where the point is a refused or unconfirmed write, and a real
  `dconf-service` on a private bus, with every XDG directory in a temporary
  directory and no activatable service, is where a change is written, read
  back, and reset. `defaults-store` is the snapshot seam, tested for round-trip,
  history, the version 1 files written before per-key capture, and every way a
  snapshot on disk can be unusable.
- `files-operations` is tested by dropping the owning UI handle mid-job and by
  injecting full disks, permission errors, and a device that disappears
  mid-copy.
- `files-gui`'s ticket 35 surfaces are tested through three seams and no host:
  a catalog built from desktop-entry fixture text, `app-catalog-platform`'s own
  `RecordingSpawner` — so a test that proves a launch proves the production path
  with only the `fork` replaced — and a fake `DeviceLink` the test drives, which
  is how a failed mount, a disconnect while viewing, and an unsafe removal are
  all reachable without a disk. `storage-service`'s client is tested against a
  real served interface over a private session bus instead, because a proxy in a
  test says nothing about the one in production.
- Localized layout coverage extends to every new GUI crate: `zh-TW` and `en-US`
  at 100%, 125%, and 150% scaling, the policy the manager GUI already follows.
- The Better OS session's seams (planned, tickets 200 to 226): every surface's
  rules live in a GPUI-free crate tested without a compositor; its desktop
  sources (toplevel info and manager, workspaces, logind, Fcitx 5,
  NetworkManager, BlueZ, the notification service) are traits with fakes, and
  D-Bus edges are tested on a private bus. A surface is seen in a nested
  `cosmic-comp` (System76's released binary extracted to a scratch directory,
  never installed, inside a headless sway under the sandbox rules: ticket 200)
  or in a disposable VM per base. `manager-daemon`'s archive steps are tested
  with the APT driver faked, the pin against a captured copy of System76's
  `Packages`, and the container check against a local mirror fixture with no
  network. Gestures are tested by replaying libinput recordings in the VM.
  Nothing of the session is installed on the owner's machine before release.

### Test matrix

| Area | Required proof |
| --- | --- |
| Manifest | valid input, missing fields, schema version, cycles, conflicts, summary limit, closed icon set, restart scope |
| Manager | list, status, declared disk-space preflight, release-note propagation, install/update/enable/disable/verify/restore lifecycle, dpkg reconciliation, real-plan validation |
| Manager store | schema preservation, atomic JSON writes, stale writers, restart resume |
| Manager platform | profile reporting, unavailable values, checksum-named artifact cache, dpkg version parsing, no mutation without an authorized privileged connection |
| Manager IPC | wire contract round-trips and every rejection case |
| Manager daemon | plan revalidation, artifact staging, APT driver, health checks, rollback outcomes, interrupted-dpkg detection from dpkg's record and its repair under its own polkit action, D-Bus surface over a private session bus |
| Monitor contracts | metric identity validation, the five distinct observation states, capability resolution, report storage, incident creation, export redaction boundary and coverage |
| Monitor collectors | CPU time categories with guest folding, load, frequency, temperature support state, memory and swap breakdown, paging units, PSI presence and absence, process identity and CPU deltas across PID reuse, command-line privacy, block-device filtering and sector units, interface counters and link attributes, truncated and malformed input, measured overhead |
| GUI | workspace build and launch smoke test on a Linux desktop, manifest-driven presentation, appearance default and migration |
| Release package | no `*-dev` in `Depends`, clean APT install, dynamic-library check, manager and monitor launch |
| App catalog | XDG discovery, each exclusion rule separately, malformed entry rejection, change watching, launch argument vector, 5,000-record benchmarks |
| App chooser | MIME section ranking, Open Once side-effect freedom, single-association `mimeapps.list` diff, rollback byte equality, executable-mode refusals |
| Launcher | matching per input field, exact/prefix over fuzzy, ranking determinism, query-driven browse/search switch, p95 query latency, no-GPUI dependency assertion |
| Launcher overlay | query-to-library round trip with no window change, grid keyboard order, launch-failure surfacing, live catalog swap keeping query and selection, locale and 100/125/150% hint overflow, headless launch smoke |
| Launcher activation | single-instance role over a fake registry and over a private session bus, toggle-versus-open verbs, unreachable owner reported rather than opening a second overlay, gesture threshold/reversal/cooldown replay, capability degradation to two paths |
| Monitor collectors | fixture-tree semantics per collector, five distinct metric states, irregular sampling intervals, per-collector overhead, source traceability records |
| Monitor views | grouping evidence and its refusals, virtualized tables at 10,000 processes, each process action and each refusal, unsupported-page honesty |
| Monitor history | collection after GUI close, store migration and corrupted-tail recovery, retention bounds, seeded-secret export redaction, CLI subcommands |
| Awake | session survives tray restart, default policy, inhibitor acquire/verify/lost/release, rule AND/OR evaluation and reason merging, battery stop, tray label fit |
| Defaults | all eight aggregate states, snapshot round-trip byte equality, external-change detection, per-entry partial failure, install does not make default |
| Touchpad | apply and read-back per control, unsupported-control presentation, config migration, gesture recognizer replay, GNOME conflict detection, no shell string reachable |
| Storage | Direct Removal default for unseen devices, readiness never claimed with pending writes, identity survives reconnect, duplicate identifiers, event-driven observation |
| Files core | typed location coverage, progressive listing off the render thread, navigation cancellation, symlink loops, encoding, long paths, case conflicts |
| Files operations | all eight job states, conflict apply-to-remaining, retry and skip, job survives window drop, final-state verification, metadata policy |
| Files GUI | 100,000-entry progressive render, virtualized scroll frame time, bookmark persistence and reorder, missing-bookmark state, hidden toggle persistence |
| Files integration | Applications location is not a directory or symlink farm, launch through the registered desktop definition, Open With routes to the chooser, all five device states in both locales, mount-on-open, disconnect cleanup leaving no history entry, unsafe-removal warning, preview cancellation and size limits and a caught parser panic, search streaming and its own hidden-file rule, manifest validation |

## Standing assumptions

- GitHub Release assets will later provide checksums and signatures, but the
  signature format is intentionally not chosen yet.
- Manifest lifecycle commands are data only. No component command is executed
  by the manager or by the privileged service. The service derives everything
  it runs from the package name, never from a manifest-supplied string.
- A manifest declares its own summary, icon, and restart scope. A component
  without a shipped translation is presented from those values rather than
  hidden. See [ADR 0006](/engineering/decisions/0006-manifest-declared-presentation.md).
- GPUI is pre-1.0 and may require the latest stable Rust and Linux display
  dependencies.
- The final runtime dependency list is target- and architecture-specific and
  must be taken from the packaged binaries rather than copied from CI's
  build-time package list.
- System76's `noble` builds of the session's 14 packages run on Zorin OS 18's
  and Ubuntu 24.04's own Wayland, Mesa, and PipeWire - risk: unproved (ADR
  0038 resolved metadata only); if false the session needs Better OS to build
  the closure itself. Ticket 201 checks it first.
- GPUI's layer-shell surface at the pinned revision works on `cosmic-comp` -
  seen in a nested `cosmic-comp` (ticket 200) at 100 %, 150 %, and 200 % once
  the fork's viewport fix is in (ADR 0043); several outputs on `cosmic-comp`
  itself are still unseen until ticket 201's VM.
- `cosmic-comp` changes behaviour between Epoch releases without notice - risk:
  a release breaks the session; ticket 226's routine holds the closure back
  until a release passes.

### Storage

No database or system migration is part of the initial scaffold. Better
Manager uses a versioned local JSON file only for mock lifecycle state. A
database remains unnecessary until concurrent multi-process history, query, or
retention requirements are explicit. Local monitor history must remain
replaceable until its storage decision is explicit.

### Linux GUI prerequisites

The GPUI desktop binaries use the Linux X11 and Wayland backends. A development
machine needs `libfontconfig1-dev`, `libxcb1-dev`, `libxkbcommon-dev`, and
`libxkbcommon-x11-dev` to link the binaries, and `libgstreamer1.0-dev` and
`libgstreamer-plugins-base1.0-dev` for Better Files' media preview; its tests
also need `gstreamer1.0-plugins-base` and `gstreamer1.0-plugins-good`. CI
installs these packages before the workspace checks. `RUST_FONTCONFIG_DLOPEN=1` avoids requiring the
fontconfig development metadata at compile time, but the runtime library is
still required when a desktop binary starts. These `*-dev` packages are
build-time dependencies only. The first release format is a `.deb`; its
metadata must declare the runtime libraries so `apt install ./<package>.deb`
works on a clean supported desktop without manual development-package setup.
See [Release Packaging Specification](/engineering/specs/release-packaging.md) for the release
verification contract.

## Decisions

- [ADR 0001: Initial Boundaries](decisions/0001-initial-boundaries.md)
- [ADR 0002: Target-Specific Release Assets](decisions/0002-release-artifact-mapping.md)
- [ADR 0003: GPL-3.0-or-later Root License](decisions/0003-project-license.md)
- [ADR 0004: Dark-First Themeable Appearance](decisions/0004-dark-first-themeable-appearance.md)
- [ADR 0005: Platform Boundary Crate](decisions/0005-platform-boundary.md)
- [ADR 0006: Manifest-Declared Presentation and Restart Metadata](decisions/0006-manifest-declared-presentation.md)
- [ADR 0007: Privileged Daemon IPC Protocol](decisions/0007-privileged-daemon-ipc.md)
- [ADR 0008: Better Launcher Five-Finger Gesture Integration](decisions/0008-launcher-gesture-integration.md)
- [ADR 0009: Better Defaults declarations, storage, and the first adapter set](decisions/0009-defaults-declarations-and-adapters.md)
- [ADR 0010: Better Awake's trigger provider set, fullscreen detection, lid-closed policy, and history retention](decisions/0010-awake-trigger-providers-and-retention.md)
- [ADR 0010: Better Touchpad ranges, configuration scope, and the dconf write path](decisions/0010-touchpad-ranges-and-dconf-writes.md)
- [ADR 0011: Better Monitor history storage — interim engine and retention](decisions/0011-monitor-history-storage-interim.md)
- [ADR 0012: The production gesture backend for Better Touchpad](decisions/0012-touchpad-gesture-backend.md)
- [ADR 0013: The refreshable component catalog](decisions/0013-remote-catalog-refresh.md)
- [ADR 0014: The privileged daemon repairs an interrupted dpkg run](decisions/0014-dpkg-interrupted-repair.md)
- [ADR 0015: The touchpad adapter names the device behind each gesture](decisions/0015-touchpad-adapter-reports-the-device.md)
- [ADR 0016: A Better Files job lives as long as the session](decisions/0016-files-job-lifetime.md)
- [ADR 0017: One control center inside Better Manager](decisions/0017-one-control-center.md)
- [ADR 0018: A shared Better OS GNOME Shell extension that only relays](decisions/0018-shared-shell-adapter-extension.md)
- [ADR 0019: Every window follows the system appearance by default](decisions/0019-appearance-follows-system.md)
- [ADR 0020: GPUI and gpui-component come from their Git repositories](decisions/0020-gpui-from-repository-sources.md)
- [ADR 0021: Release packages are built per Ubuntu release and architecture on native runners](decisions/0021-release-build-environment.md)
- [ADR 0022: Better Manager executes for real by default](decisions/0022-real-execution-by-default.md)
- [ADR 0023: Better Monitor reads /proc and /sys itself and keeps five value states](decisions/0023-monitor-reads-proc-and-sys.md)
- [ADR 0024: The installed-artifact record selects the rollback target](decisions/0024-installed-artifact-selects-rollback.md)
- [ADR 0025: Better Launcher is one instance on org.betteros.Launcher1](decisions/0025-launcher-single-instance.md)
- [ADR 0026: The Ready to unplug rule and its two thresholds](decisions/0026-storage-readiness-rule.md)
- [ADR 0027: Better Storage stays unprivileged](decisions/0027-storage-privileged-boundary.md)
- [ADR 0028: The privileged service is upgraded outside its own unit](decisions/0028-daemon-upgrades-outside-its-own-unit.md)
- [ADR 0029: Hardware writes happen in a helper unit that sees one file](decisions/0029-hardware-writes-in-a-helper-unit.md)
- [ADR 0030: Touchpad scroll speed through an evdev/uinput pass-through](decisions/0030-touchpad-scroll-passthrough.md)
- [ADR 0031: Touchpad scroll speed needs a privileged input pass-through](decisions/0031-touchpad-scroll-speed.md)
- [ADR 0032: Which drives mount automatically is kept in fstab and a udev rule](decisions/0032-drive-mounting-choices.md)
- [ADR 0033: A login screen watch restarts a hung GDM once per boot](decisions/0033-login-screen-watch.md)
- [ADR 0035: The touchpad adapter's suppression follows the relay rule](decisions/0035-touchpad-adapter-relay-rule.md) (proposed)
- [ADR 0036: The top bar is a Better OS window that the shell makes a dock](decisions/0036-top-bar-dock-surface.md)
- [ADR 0037: Better Input's engine is Rust on libchewing, with 0c's MIT models](decisions/0037-better-input-engine.md) (proposed)
- [ADR 0038: Better OS's desktop: a Better OS session on COSMIC's compositor, beside GNOME](decisions/0038-cosmic-as-the-desktop.md)
- [ADR 0039: How Better OS keeps its patch to cosmic-comp](decisions/0039-compositor-fork-policy.md) (proposed)
- [ADR 0040: What Better OS is, now that it has a session of its own](decisions/0040-what-better-os-is.md) (proposed)
- [ADR 0043: Better OS builds GPUI from its own fork of Zed](decisions/0043-gpui-from-a-fork.md)
- [ADR 0045: Better OS moves to GPUI Kit's current release, on GPUI Fast](decisions/0045-gpui-kit-on-gpui-fast.md) (the upgrade is ticket 257)
- [ADR 0046: The USB-C recovery reloads one signed module and rebinds one controller, in helper units](decisions/0046-usb-c-recovery-through-the-daemon.md) (proposed)

## Per-package programs run as the person

A package may ship two programs Better Manager runs as the person, for what
the component changed in the person's session that dpkg cannot see:

- `/usr/lib/better-os/before-removal/<package>`: right before the daemon
  removes the package (after its units stop), and with `--undo` when the
  daemon then refuses the removal (ticket 88).
- `/usr/lib/better-os/on-disable/<package>`: when the component is turned
  off (after its units and extensions), and with `--undo` when it is turned
  on again (ticket 129).

Both are found only in dpkg's own file list for the package, never at a path
a manifest names, and are run only when they are a regular file (not a link)
owned by root that neither group nor others can write
(`manager_platform::removal_hooks::refuse_program`). Neither blocks the
operation: exit 0 is done, exit 3 is a warning in Activity, exit 4 means
there was no session to write in and a pending marker that Better Manager
finishes at its next start in the person's session, by the record
(installed, on or off). Tests never run them while
`BETTER_OS_TEST_SYSTEMCTL_GUARD` is set. Ticket 176 makes this one
mechanism for every component's defaults.
