# Engineering Plan: Better OS

## Architecture

```text
component manifests ──> better-core validation ──> manager-core
                                                   plan + mock lifecycle state
                       manager-platform ──────────────┘  ├── manager-cli ──┐
                       capability/download/package       └── manager-gui ──┼── manager-store JSON
                       privileged executor (refuses)                       └── versioned local state

monitor-collectors-linux (/proc, /sys) ──> monitor-core metric contracts,
                                           reports/incidents/export ──> monitor-gui

better-ui provides GPUI presentation primitives to both GUI crates.
Privileged execution lives in `manager-daemon`, a separate root process reached
over the D-Bus system bus and authorized by polkit. See ADR 0007.

Release builds package the final GUI binaries as `.deb` assets. Package
metadata declares runtime dependencies; the development-only GPUI linker
packages stay in the build environment and are never a user installation
prerequisite.
```

## Component suite architecture

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
        └──> files-core ─────────> files-gui           Applications location

storage-core ──> storage-platform ──> storage-service ──> files-gui
   device identity, Direct Removal policy, state machine, typed device states

files-core ──> files-platform ──> files-operations ──> files-gui
   typed locations   fs/XDG/MIME/trash   durable job engine   window, views

monitor-core ──> monitor-collectors-linux ──> monitor-service ──> monitor-gui
   typed metric/capability contracts     monitor-ipc / monitor-store / monitor-cli

awake-core ──> awake-platform ──> awake-service ──> awake-ipc ──┬── awake-tray
   sessions, policies, trigger rules   owns inhibitors          └── awake-gui
                                       awake-store

touchpad-core ──> touchpad-platform ──> touchpad-gui
   versioned config     device/session adapters
touchpad-gestures ──> touchpad-session ──> better-actions ──> launcher-gui
   gesture model, preset, conflicts    typed desktop actions

defaults-core ──> defaults-platform ──> defaults-store ──> manager-gui / CLI
   declarations, aggregate status, plans   typed adapters   snapshots + history
```

`better-ui` provides GPUI presentation primitives to every GUI crate in the
suite, not only the two that exist today.

## Component seams

- **One catalog, no second scanner.** `app-catalog-core` and
  `app-catalog-platform` are the only place a `.desktop` file is parsed. The
  chooser, the launcher index, and the Applications location consume records.
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
  run a shell pipeline, or hold a privileged handle.
- **Five states, not one number.** Monitor metric contracts distinguish unknown,
  unsupported, permission denied, stale, and zero. Storage distinguishes Ready to
  unplug, Writing, Busy, Performance, and Unknown. Defaults distinguishes eight
  aggregate states. In all three, an unverifiable condition is reported, never
  rendered as a reassuring value.
- **The service owns the token.** `awake-service` holds the inhibitor;
  `awake-tray` and `awake-gui` are clients that can restart without ending a
  session. The tray executes no shell command.
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
  nothing else. See [ADR 0008](docs/decisions/0008-defaults-declarations-and-adapters.md).
- **Gesture logic stays in Rust core crates.** `touchpad-gestures` owns the
  model, preset, and conflict detection; `touchpad-session` is a narrow adapter
  that invokes typed `better-actions` and never accepts an arbitrary shell
  string from configuration. The launcher's gesture seam is the same boundary
  seen from the other side.

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

## Test seams

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
  observation states stay distinct. Collector behavior is tested against
  captured `/proc` and `/sys` fixture trees through the `Roots` seam, including
  truncated and malformed input, two-sample deltas, a kernel without
  `CONFIG_PSI`, and an unreadable descriptor directory. Collection cost is
  measured rather than asserted from a budget.
- `manager-gui` uses the demo catalog and state to assert that its Update All
  path produces the same dry-run plan as `manager-core`, that every catalog
  component is presentable from its manifest with no hardcoded component IDs,
  that a pre-theme state file loads dark, and both locales plus 100/125/150%
  long-label layout policy.
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
  D-Bus tests.
- `defaults-platform` ships a mock adapter for every declared integration kind,
  so aggregate status, partial failure, and external-change detection are all
  provable before a real adapter exists for that kind. Its two real adapters are
  tested against recorded input rather than the running desktop: `mimeapps.list`
  content for the XDG adapter, and a `dconf compile` fixture database for the
  GNOME one. `defaults-store` is the snapshot seam, tested for round-trip,
  history, and every way a snapshot on disk can be unusable.
- `files-operations` is tested by dropping the owning UI handle mid-job and by
  injecting full disks, permission errors, and a device that disappears
  mid-copy.
- Localized layout coverage extends to every new GUI crate: `zh-TW` and `en-US`
  at 100%, 125%, and 150% scaling, the policy the manager GUI already follows.

## Test matrix

| Area | Required proof |
| --- | --- |
| Manifest | valid input, missing fields, schema version, cycles, conflicts, summary limit, closed icon set, restart scope |
| Manager | list, status, declared disk-space preflight, release-note propagation, install/update/enable/disable/verify/restore lifecycle, dpkg reconciliation, real-plan validation |
| Manager store | schema preservation, atomic JSON writes, stale writers, restart resume |
| Manager platform | profile reporting, unavailable values, checksum-named artifact cache, dpkg version parsing, no mutation without an authorized privileged connection |
| Manager IPC | wire contract round-trips and every rejection case |
| Manager daemon | plan revalidation, artifact staging, APT driver, health checks, rollback outcomes, D-Bus surface over a private session bus |
| Monitor contracts | metric identity validation, the five distinct observation states, capability resolution, report storage, incident creation, export redaction boundary and coverage |
| Monitor collectors | CPU time categories with guest folding, load, frequency, temperature support state, memory and swap breakdown, paging units, PSI presence and absence, process identity and CPU deltas across PID reuse, command-line privacy, block-device filtering and sector units, interface counters and link attributes, truncated and malformed input, measured overhead |
| GUI | workspace build and launch smoke test on a Linux desktop, manifest-driven presentation, appearance default and migration |
| Release package | no `*-dev` in `Depends`, clean APT install, dynamic-library check, manager and monitor launch |
| App catalog | XDG discovery, each exclusion rule separately, malformed entry rejection, change watching, launch argument vector, 5,000-record benchmarks |
| App chooser | MIME section ranking, Open Once side-effect freedom, single-association `mimeapps.list` diff, rollback byte equality, executable-mode refusals |
| Launcher | matching per input field, exact/prefix over fuzzy, ranking determinism, query-driven browse/search switch, p95 query latency, no-GPUI dependency assertion |
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
| Files integration | Applications location is not a directory or symlink farm, Open With routes to the chooser, disconnect cleanup, preview cancellation and size limits |

## Migration plan

No database or system migration is part of the initial scaffold. Better
Manager uses a versioned local JSON file only for mock lifecycle state. A
database remains unnecessary until concurrent multi-process history, query, or
retention requirements are explicit. Local monitor history must remain
replaceable until its storage decision is explicit.

## Hidden assumptions

- GitHub Release assets will later provide checksums and signatures, but the
  signature format is intentionally not chosen yet.
- Manifest lifecycle commands are data only. No component command is executed
  by the manager or by the privileged service. The service derives everything
  it runs from the package name, never from a manifest-supplied string.
- A manifest declares its own summary, icon, and restart scope. A component
  without a shipped translation is presented from those values rather than
  hidden. See [ADR 0006](docs/decisions/0006-manifest-declared-presentation.md).
- GPUI is pre-1.0 and may require the latest stable Rust and Linux display
  dependencies.
- The final runtime dependency list is target- and architecture-specific and
  must be taken from the packaged binaries rather than copied from CI's
  build-time package list.
