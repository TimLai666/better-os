# Delivery Status

## Current Phase

Expanding Better OS from Manager to the full component suite

The previous phase — Better Manager applying real component transactions,
including rollback after a real mutation failure — is complete through
milestone M20.

## Stage Objective

Build the components Better Manager exists to install: the shared application
catalog, Better Launcher, Better Monitor's real collectors, Better Awake,
Better Defaults, Better Touchpad, safe direct-removal external storage, and
Better Files — each independently versioned, each honest about what it cannot
observe or change, and none of them reimplementing what another already owns.

The previous objective is met: Better Manager installs, updates, removes, and
rolls back first-party components without weakening the boundary that keeps
privileged mutation out of the GUI and CLI.

## Active Workstreams

- Shared manifest schema and validation
- Manager dry-run planning and CLI
- Better Manager GPUI shell, shared mock lifecycle, persistence, and acceptance coverage
- Manifest-declared presentation, platform boundary, and dark-first appearance
- Monitor metric contracts and Linux collectors
- Release packaging contract, clean-install verification, and license notices
- Privileged daemon IPC contract, real artifact download, and APT execution
- Shared application catalog and Better App Chooser (Issue #4)
- Better Launcher unified overlay (Issue #2)
- Better Monitor real collectors, task manager, and history (Issue #16)
- Better Awake tray sessions and trigger rules (Issue #13)
- Better Defaults preview, apply, and restore (Issue #10)
- Better Touchpad control center and Mac-style gestures (Issue #3)
- Safe direct-removal external storage (Issue #5)
- Better Files file manager (Issue #6)

## Milestones

| id | target | owner | status | verification_signal |
| --- | --- | --- | --- | --- |
| M1 | workspace and shared contracts | agent | done | `cargo test -p better-core` |
| M2 | manager dry-run path | agent | done | CLI list/status/plan output |
| M3 | monitor and GUI shells | agent | done | `cargo build --workspace`; both GUI binaries stayed alive for 8 seconds in a Wayland session |
| M4 | docs and CI | agent | done | workflow file and docs review |
| M5 | release packaging contract | agent | done | `docs/release-packaging.md` and ticket 06 acceptance criteria |
| M6 | target-compatible `.deb` packaging | agent | done | GitHub Actions Ubuntu 22.04/24.04 amd64 and native arm64 matrix build, dependency metadata check, checksum verification |
| M7 | package license notices | agent | done | generated locked Cargo inventory, package payload notice checks, and post-merge CI verifier |
| M8 | v0.1.0 public release | agent | done | GitHub Release assets, public re-download checksum verification, and manifest checksum mapping |
| M9 | Better Manager Issue #8 functional acceptance | agent | done | Chefer AppCipe passed fmt, workspace check/test, clippy, CLI lifecycle smoke, and GUI headless smoke |
| M10 | Issue #8 remaining gap closure | agent | done | manifest presentation and restart metadata, `manager-platform`, manifest-driven GUI, dark-first appearance, ADRs 0004-0006 |
| M11 | privileged IPC decision and wire contract | agent | done | ADR 0007, `manager-ipc` with 24 rejection and round-trip tests, `cargo fmt`/check/test/clippy `-D warnings` |
| M12 | core execution seam and state schema v2 | agent | done | lifecycle suite green through the mock driver, v1 state migration, real-plan validation |
| M13 | privileged daemon | agent | done | 39 unit tests against fake APT/host/health, 6 private session-bus tests, daemon `.deb` with unit, polkit policy, and bus config |
| M14 | real download, dpkg reconciliation, and D-Bus client | agent | done | checksum-named artifact cache, drift detection blocking planning, CLI `--execution real` reporting `daemon.unavailable` |
| M15 | GUI real execution | agent | done | background transaction with live progress, cancel offered only while honorable, real failure copy in both locales |
| M16 | cutover and documentation | agent | done | real execution by default, daemon packaging verified, AGENTS/ENG/README/architecture/security updated |
| M17 | container end-to-end verification | agent | done | `chefer run packaging/e2e/appcipe.yml`: real dpkg, real system bus, real polkitd, unauthorized request refused |
| M18 | four-way packaging matrix on CI | agent | done | CI run 30688730458: build, verify, and container end-to-end passed on ubuntu 22.04/24.04 × amd64/arm64 |
| M19 | authorized transaction verified end to end | agent | done | real apt install and removal through the service, confirmed against dpkg; a deadlock in the D-Bus client found and fixed |
| M20 | rollback target correctness and mutation-failure coverage | agent | done | installed-artifact record replaces the guessed rollback target; container run proved a failed update reinstalls 0.0.9, not the version that just failed |
| M21 | ticket 18 — shared app catalog core and platform | agent | done | workspace gate passed; 5,000-record benchmarks (cold 44.6 ms, warm 40.4 ms, 20.1 MB) and a launch smoke proving the argument vector was never shell-interpreted |
| M22 | ticket 19 — Better App Chooser (needs M21) | agent | done | workspace gate passed; `mimeapps.list` single-association diff and rollback byte equality proven over six fixture shapes; headless chooser smoke in both modes |
| M23 | ticket 20 — launcher-core index and ranking (needs M21) | agent | done | crate-scoped fmt/check/test/clippy gate, 53 tests, and 5,000-record benchmarks: query latency p95 1.005 ms against the 50 ms target, cold index build 17.8 ms; full workspace gate runs after merge |
| M24 | ticket 21 — launcher overlay, activation, gesture ADR (needs M23) | agent | todo | workspace gate plus headless overlay smoke, manifest validation, and the gesture-options ADR |
| M25 | ticket 22 — monitor metric contracts and Linux collectors | agent | done | typed metric/capability contracts with five distinct observation states, six `/proc` and `/sys` collectors, 157 tests against captured fixture trees, measured overhead 10.2 ms/round for 1,359 tasks; full workspace gate ran after merge |
| M26 | ticket 23 — Apps, Processes, actions, real Overview (needs M25) | agent | done | workspace gate green; 10,000-process benchmark published (adopt a round 1.7 ms, group into applications 12.4 ms); locale/scaling overflow tests pass in both languages at 100/125/150%; a real child process was stopped, resumed, reniced, and terminated through the typed action interface |
| M27 | ticket 24 — monitor service, history, incidents, export, CLI (needs M25) | agent | todo | workspace gate plus collection-after-GUI-close smoke and a seeded-secret export test |
| M28 | ticket 25 — Better Awake tray-first manual sessions | agent | done | crate-scoped fmt/check/test/clippy gates, 142 tests including 11 private session-bus tests, a tray-restart session survival test, and an 8 s headless `awake-gui` smoke |
| M29 | ticket 26 — Awake full application and trigger rules (needs M28) | agent | todo | workspace gate plus rule-engine evaluation tests and an uninstall smoke releasing inhibitors |
| M30 | ticket 27 — defaults core, adapters, snapshots, CLI | agent | todo | workspace gate plus snapshot round-trip and external-change detection tests |
| M31 | ticket 28 — Manager Defaults GUI review flows (needs M30) | agent | todo | workspace gate plus a preview-before-mutation assertion and locale/scaling overflow tests |
| M32 | ticket 29 — Better Touchpad pointer, scrolling, clicking, devices | agent | todo | workspace gate plus apply-and-read-back tests per control and input-latency benchmarks |
| M33 | ticket 30 — Mac-style gestures, typed actions, backend ADR (needs M32) | agent | todo | workspace gate plus recognizer replay tests, conflict detection, and the gesture backend ADR |
| M34 | ticket 31 — safe direct-removal external storage | agent | in review on `ticket-31`, not merged | crate-scoped gate plus event-sequence state-machine tests and synthetic state/latency benchmarks; the workspace gate and hardware flush-completion benchmarks are still outstanding |
| M35 | ticket 32 — files-core typed locations and navigation (needs M21, M34) | agent | todo | workspace gate plus 100,000-entry listing benchmarks and a navigation cancellation test |
| M36 | ticket 33 — files-operations durable job engine (needs M35) | agent | todo | workspace gate plus a job-survives-window-drop test and copy/move benchmarks |
| M37 | ticket 34 — files-gui window, sidebar, views, operations (needs M36) | agent | todo | workspace gate plus the 100,000-entry progressive render benchmark and bookmark persistence tests |
| M38 | ticket 35 — Applications, devices, preview, search, benchmarks (needs M22, M37) | agent | todo | workspace gate plus the full Better Files benchmark harness and manifest validation |

Every milestone from M21 onward shares the same base gate: `cargo fmt --all --
--check`, `cargo check --workspace`, `cargo test --workspace`, and `cargo clippy
--workspace --all-targets -- -D warnings`. The signal column names only what
each ticket adds on top.

## Current Blockers

Better Manager now records the installed artifact that belongs to each
successful package change, instead of guessing a rollback target from the
artifact it is about to install. That guess was a real defect: a failed update
would have reinstalled the version that just failed and reported the host as
restored. The container run confirms the fix — a failed update now ends with
`dpkg-query` reporting 0.0.9 and the record pointing at the 0.0.9 artifact.

CI run 30688730458 previously ran the authorized install/removal check on all
four supported combinations —
ubuntu 22.04 and 24.04, amd64 and arm64 — and every one passed, including the
service claiming its bus name and refusing an unauthorized request on native
arm64 hardware.

The privileged daemon IPC protocol, which `AGENTS.md` required to be decided
before any real system installation or rollback, is decided in ADR 0007: a
D-Bus system service authorized by polkit. Package signing and the public APT
repository stay deferred by explicit scope choice, not by omission; the
manager verifies artifact checksums instead.

No active blocker remains for Ticket 06. The target-specific asset naming and
manifest mapping decision is recorded in ADR 0002, the release packaging changes
are merged into `main`, and the first public release is available.

Issue #8 has no active implementation blocker either. Every acceptance
criterion in the issue is met, and the five remaining gaps from the branch
audit are closed under ticket 08.

The declared Rust 1.85 baseline is still incompatible with the current
lockfile: an isolated Rust 1.85 build stops before compilation because
dependencies now require up to Rust 1.92. The GitHub workflow uses stable Rust.
The Issue #8 Chefer AppCipe passed with Rust 1.97. The supported toolchain
policy still needs alignment.

## Next Verifiable Output

Better Monitor's historical collection surviving a GUI close, with a versioned
store and a redacted export (ticket 24), and `launcher-core` matching and
ranking over the shared catalog with its p95 latency benchmark (ticket 20).

The Better Manager follow-ups remain open and unscheduled: package signing, the
public APT repository, a repair action for a transaction interrupted mid-flight,
and aligning the declared Rust baseline with what the lockfile actually
requires.

## Next Ticket

Tickets 18, 19, 22, and 23 are done; 31 is implemented and in review on its own
branch. Ready now (blockers met): 20 (needs 18), 24 (needs 22), 26 (needs 25,
in progress), 27, and 29. Remaining
dependency edges, in ticket order: 21 needs 20; 28 needs 27; 30 needs 29; 32
needs 18 and 31; 33 needs 32; 34 needs 33; 35 needs 19 and 34.

## Decision Log

- decision: use a Rust workspace with separate core, CLI, GUI, and monitor crates
  rationale: preserve non-privileged and presentation boundaries from issue #1
  timestamp: 2026-07-31
  impacted_ticket_ids: [01, 02, 03]
- decision: keep `RUST_FONTCONFIG_DLOPEN=1` in local and CI checks, and install
  X11/Wayland development libraries in CI
  rationale: GPUI's current Linux backend links fontconfig, XCB, and xkbcommon;
  the project should expose that prerequisite instead of hiding a failed GUI link
  timestamp: 2026-07-31
  impacted_ticket_ids: [04, 05]
- decision: use GitHub repository sources for GPUI and gpui-component during the
  scaffold because the upstream README documents that integration path
  rationale: GPUI is pre-1.0 and the current component README uses repository
  dependencies
  timestamp: 2026-07-31
  impacted_ticket_ids: [04]
- decision: keep GPUI `*-dev` packages in the build environment and declare only
  verified runtime libraries in each release `.deb`
  rationale: release users should install through local APT without setting up a
  compiler/linker environment; package metadata is the correct dependency
  boundary
  timestamp: 2026-07-31
  impacted_ticket_ids: [06]
- decision: run arm64 release packaging on native GitHub-hosted arm64 runners
  rationale: the component manifests declare amd64 and arm64 support, and native
  runners avoid QEMU emulation while preserving one compatible build environment
  per Ubuntu release and architecture
  timestamp: 2026-07-31
  impacted_ticket_ids: [06]
- decision: use one version Release with target-specific package assets and
  schema v2 artifact variants keyed by Ubuntu release and architecture
  rationale: avoid filename collisions across the four package jobs and let
  every manifest checksum map to exactly one published package
  timestamp: 2026-07-31
  impacted_ticket_ids: [06]
- decision: license the root project under GPL-3.0-or-later
  rationale: make the project license explicit before distribution and align the
  first-party workspace with the GPL-3.0-or-later GUI dependency chain
  timestamp: 2026-07-31
  impacted_ticket_ids: [05, 06]
- decision: use the GitHub account contact as Debian maintainer
  rationale: provide a reachable package contact without inventing a project
  mailbox that does not exist yet
  timestamp: 2026-07-31
  impacted_ticket_ids: [06]
- decision: ship the root license and generated Cargo dependency license
  inventory inside every Debian package
  rationale: make the current locked dependency graph and its copyleft or
  notice-sensitive records reviewable from the binary distribution, while
  failing the package build if the committed inventory becomes stale
  timestamp: 2026-08-01
  impacted_ticket_ids: [06]
- decision: persist Better Manager's mock lifecycle in versioned local JSON and
  show disk or release metadata only when its catalog declares it
  rationale: the user chose JSON over a database; explicit unavailable values
  preserve truthful review screens without inventing release data
  timestamp: 2026-07-31
  impacted_ticket_ids: [07]
- decision: open Better Manager dark by default and keep light and
  system-follow as explicit stored choices
  rationale: Issue #8 states a dark-first UI direction, and the shipped
  light-only slice contradicted it; see ADR 0004
  timestamp: 2026-08-01
  impacted_ticket_ids: [08]
- decision: move every host-facing interface into `manager-platform` and keep
  each shipped implementation a mock that refuses to apply a package change
  rationale: Issue #8 lists the crate boundary and requires the privileged
  executor to stay an interface until its security design is approved; a mock
  that returned success would claim the host changed; see ADR 0005
  timestamp: 2026-08-01
  impacted_ticket_ids: [08]
- decision: declare component summary, icon, and restart scope in the manifest
  with a closed icon set, and present untranslated components from those values
  rationale: the GUI matched three hardcoded component IDs and dropped every
  other component; manifests are untrusted, so the icon set stays closed; see
  ADR 0006
  timestamp: 2026-08-01
  impacted_ticket_ids: [08]

- decision: make the privileged boundary a D-Bus system service authorized by
  polkit, implemented with zbus
  rationale: the target desktop already runs polkit and the system bus for every
  privileged desktop operation, so the authentication prompt and activation are
  provided and already audited rather than reimplemented; see ADR 0007
  timestamp: 2026-08-01
  impacted_ticket_ids: [09, 11, 14]
- decision: carry plans and outcomes as JSON documents defined once in a shared
  `manager-ipc` crate, with the client downloading artifacts and the daemon
  re-hashing what it receives over a file descriptor
  rationale: one definition for both sides beats a hand-matched D-Bus encoding
  for a schema this deep, and keeping TLS out of the root process costs nothing
  in integrity because the daemon must treat the client as untrusted anyway
  timestamp: 2026-08-01
  impacted_ticket_ids: [09, 11, 12]
- decision: make real execution the default for both the CLI and the GUI, with
  an explicit demo mode that says so on screen
  rationale: a manager that quietly simulated would report a change that never
  happened; a missing privileged service is an error, not a reason to pretend
  timestamp: 2026-08-01
  impacted_ticket_ids: [13, 14]
- decision: offer cancellation only while the transaction is still downloading
  rationale: once the plan has gone to the privileged service the host may
  already have changed, and a cancel button there would promise a restoration
  nothing performed
  timestamp: 2026-08-01
  impacted_ticket_ids: [10, 13]
- decision: keep package signing and the public APT repository deferred while
  real installation lands
  rationale: the user scoped this round to real execution; published checksums
  are already the integrity mechanism and signing needs a key custody decision
  of its own
  timestamp: 2026-08-01
  impacted_ticket_ids: [09, 14]
- decision: make "no value" a five-way distinction in the monitor contract —
  unknown, unsupported, permission denied, stale, and a measured zero — instead
  of `Option`
  rationale: a monitor that collapses them tells the user a machine is idle when
  it is actually unobserved; PSI on a kernel without `CONFIG_PSI` and a
  descriptor count on another user's process are the cases that forced it
  timestamp: 2026-08-30
  impacted_ticket_ids: [22]
- decision: read `/proc` and `/sys` directly rather than adopting `sysinfo` for
  the metrics in ticket 22
  rationale: the eight CPU time categories, PSI, vmstat paging counters,
  diskstats queue and service time, and per-process cgroup paths are either
  absent from a portable abstraction or flattened by it, and a portable API
  reports zero where an interface is missing; revisiting it for battery,
  component naming, and disk identity stays open
  timestamp: 2026-08-30
  impacted_ticket_ids: [22]
- decision: give every collector a `Roots` parameter so no path is hardcoded
  rationale: tests then drive the production read path against captured `/proc`
  snapshots instead of a parser in isolation, which is what caught the
  `/proc/net/dev` column-padding and AMD per-core temperature cases
  timestamp: 2026-08-30
  impacted_ticket_ids: [22]
- decision: make the installed artifact record authoritative for rollback target selection
  rationale: a transaction rollback record describes one prior transaction, while
  only a version-matched component record can prove which cached artifact produced
  the version dpkg currently reports; the new artifact must never be a fallback
  for an old version
  timestamp: 2026-08-02
  impacted_ticket_ids: [17]

## Source Links

- [Issue #1](https://github.com/TimLai666/better-os/issues/1)
- [Issue #2: Better Launcher](https://github.com/TimLai666/better-os/issues/2)
- [Issue #3: Better Touchpad](https://github.com/TimLai666/better-os/issues/3)
- [Issue #4: Applications view and Better App Chooser](https://github.com/TimLai666/better-os/issues/4)
- [Issue #5: Safe direct-removal external storage](https://github.com/TimLai666/better-os/issues/5)
- [Issue #6: Better Files](https://github.com/TimLai666/better-os/issues/6)
- [Issue #8](https://github.com/TimLai666/better-os/issues/8)
- [Issue #10: Better Defaults](https://github.com/TimLai666/better-os/issues/10)
- [Issue #13: Better Awake](https://github.com/TimLai666/better-os/issues/13)
- [Issue #16: Better Monitor](https://github.com/TimLai666/better-os/issues/16)
- [Monitor collector source traceability](docs/monitor-collector-sources.md)
- [ENG.md](ENG.md)
- [Architecture](docs/architecture.md)
- [Release packaging](docs/release-packaging.md)
- [ADR 0002: Target-specific release assets](docs/decisions/0002-release-artifact-mapping.md)
- [ADR 0003: GPL-3.0-or-later root license](docs/decisions/0003-project-license.md)
- [ADR 0004: Dark-first themeable appearance](docs/decisions/0004-dark-first-themeable-appearance.md)
- [ADR 0005: Platform boundary crate](docs/decisions/0005-platform-boundary.md)
- [ADR 0006: Manifest-declared presentation](docs/decisions/0006-manifest-declared-presentation.md)
- [ADR 0007: Privileged daemon IPC protocol](docs/decisions/0007-privileged-daemon-ipc.md)
- [Third-party license inventory](docs/third-party-licenses.md)
- [Pull request #15](https://github.com/TimLai666/better-os/pull/15)
- [v0.1.0 release](https://github.com/TimLai666/better-os/releases/tag/v0.1.0)
- [Pull request #9](https://github.com/TimLai666/better-os/pull/9)
- [Pull request #11](https://github.com/TimLai666/better-os/pull/11)
- [Pull request #12](https://github.com/TimLai666/better-os/pull/12)
- [CI run 30616628027](https://github.com/TimLai666/better-os/actions/runs/30616628027)
- [CI run 30625827340](https://github.com/TimLai666/better-os/actions/runs/30625827340)
- [Main CI run 30631266909](https://github.com/TimLai666/better-os/actions/runs/30631266909)
- [Main CI run 30638535908](https://github.com/TimLai666/better-os/actions/runs/30638535908)
- [Main CI run 30650287246](https://github.com/TimLai666/better-os/actions/runs/30650287246)
- [Tickets](docs/tickets/)
- [Decisions](docs/decisions/)

## Handoff Notes

The checkout started with only `README.md`. Rust is available through
`/home/tim/.cargo/bin`, but is not on the default shell `PATH`.

The four-entry Ubuntu 22.04/24.04 amd64 and native arm64 package matrix passed
in PR #11, including the native `dpkg`/`uname` checks, package build, runtime
dependency checks, checksums, and artifact upload. Clean Ubuntu containers for
all four target/architecture pairs installed the downloaded artifacts with APT,
had no `*-dev` packages, resolved all dynamic libraries, and passed checksum
verification. Both GUI binaries stayed alive in GPUI headless mode.
The post-merge `main` CI run 30650287246 passed Rust checks and all four package
jobs. PR #15 is merged into `main` at `3a6d98b`; the release artifact mapping
decision is recorded in ADR 0002, and `main` validates schema v2 manifests and
emits target-specific package filenames.
The package payload also started both GUI binaries for 12 seconds in the host's
Zorin OS 18.1 GNOME Wayland session with `ZED_HEADLESS` unset. Docker's Xvfb and
host-socket tests still failed because they did not provide a usable compositor,
but the direct host session passed. The public `v0.1.0` release contains eight
target-specific `.deb` assets and eight sidecars, and all public sidecars were
verified after re-download. Both release-eligible manifests now contain the
actual package checksums. Local `cargo fmt`, `cargo check`, `cargo clippy`,
workspace tests, package build, package verifier, and inventory freshness checks
also passed.
The approved Debian maintainer is `TimLai666 <tim930102@icloud.com>`, and the
root project license is GPL-3.0-or-later.
PR #9 is merged into `main` at `fb3520f`. PR #12 and PR #15 feature branches
have been deleted from both the local checkout and GitHub.

Issue #8 now keeps the CLI and GPUI on the shared `manager-core` lifecycle API,
persists mock state through `manager-store`, and localizes the GUI at runtime.
The final disposable Chefer AppCipe passed `cargo fmt --all -- --check`,
`cargo check --workspace --offline --quiet`, `cargo test --workspace --offline
--quiet`, workspace clippy with `-D warnings`, CLI lifecycle smoke, and an
8-second `ZED_HEADLESS=1` GUI launch smoke. No privileged command or real
package mutation ran.
The temporary `better-manager-gpui-complete/` directory was moved to the
desktop trash after the destination file set was verified.

Real system integration is planned as tickets 09 through 14. Ticket 09 is done:
ADR 0007 records the D-Bus and polkit decision with its rejected alternatives
and its accepted residual risk, ADR 0005 no longer claims the protocol is
undecided, and `manager-ipc` holds the wire contract both halves will share.
Local `cargo fmt --all -- --check`, `cargo check --workspace --offline`,
`cargo test --workspace --offline`, and `cargo clippy --workspace --all-targets
--offline -- -D warnings` all passed; `manager-ipc` contributes 24 tests, mostly
rejection cases. No daemon, no download code, and no APT invocation exists yet,
so nothing in this branch can still apply a package change and the shipped
backends all continue to refuse.

Tickets 09 through 14 are done. Ticket 10 is done. `Manager::advance` now consumes a `StageOutcome` a driver
produced rather than one the caller scripted; `advance_mock` translates the old
scripted vocabulary into the same outcomes, which is why the 20 existing
lifecycle tests still pass unchanged in meaning. Plan steps, component records,
and snapshots carry artifact identity, so a real transaction can say what it is
installing and a restore has something verifiable to reinstall. State schema
version 2 migrates version 1 files in place instead of quarantining them, and a
version 1 restore point is honest that it does not know which artifact produced
it: simulations still offer that restore, real transactions refuse it. Local
`cargo fmt --all -- --check`, `cargo check --workspace --offline`, `cargo test
--workspace --offline`, and `cargo clippy --workspace --all-targets --offline --
-D warnings` all passed, along with CLI install and `--fail-at installing`
lifecycle smokes against a disposable state file.

A branch audit against the Issue #8 text then found five gaps outside the
acceptance-criteria list: no dark theme, no `manager-platform` crate,
`replaces`/`enhances` never surfaced, component icon and purpose hardcoded to
three component IDs, and a `RestartRequirement` with only `NotDeclared`. All
five are closed under ticket 08, with ADRs 0004, 0005, and 0006 recording the
decisions Issue #8 asked to be written down rather than made silently. The
hardcoded-ID map also silently dropped any component outside that list from
the GUI; presentation is now manifest-driven, so it does not.

Tickets 11 through 14 completed the real path. `manager-daemon` is a D-Bus
system service authorized by polkit that revalidates every plan from scratch
against the host, applies it through local APT, health-checks what it applied,
and rolls back what it can. `manager-platform` fetches artifacts into a cache
named by checksum and reads installed versions from dpkg. The GUI runs the
transaction off the UI thread and offers cancellation only while it can still
be honored. Real execution is the default for both front ends; demo mode is
explicit and visible.

Local `cargo fmt --all -- --check`, `cargo check --workspace --offline`,
`cargo test --workspace` (21 suites, no failures), and `cargo clippy --workspace
--all-targets -- -D warnings` all passed. `packaging/build-deb.sh` and
`packaging/verify-deb.sh` built and verified all three packages for
ubuntu-24.04 on amd64, including the daemon's unit, polkit policy, and bus
config. A `ZED_HEADLESS=1` GUI launch stayed alive for the full smoke window.
CLI smokes confirmed that real mode without a daemon reports
`daemon.unavailable` and writes no state, that `--execution mock` still walks
the old lifecycle, and that reconciliation detects a recorded component dpkg
has never heard of and blocks planning for it.

What has not run: `packaging/test-daemon-e2e.sh` inside a container, and the
four-way release/architecture packaging matrix. The daemon has never faced a
real polkit or a real dpkg. That is milestone M17.

Ticket 15 ran the end-to-end check for real. `chefer run
packaging/e2e/appcipe.yml` builds a disposable Ubuntu 24.04 image and, inside
it: installs `better-manager-daemon` and confirms the unit, bus config, polkit
policy, and state directories land where they should; installs, removes, and
reinstalls `better-monitor` through apt, asserting dpkg state each time; starts
a real system bus and polkitd and the real service binary; confirms the service
claims `org.betteros.Manager1` and reports protocol version 1 without
authorization; confirms an unauthorized `ApplyTransaction` is refused and
leaves no transaction journal; and confirms a purge removes
`/var/lib/better-os` and `/var/cache/better-os`. It exits 0.

Two things that run found. The daemon package installed into an image with no
graphics libraries at all, which is the packaging split doing its job. And the
Chefer sandbox has no network, so anything apt needs at test time has to be in
the image already — the desktop libraries `better-monitor` depends on are
installed at build time for that reason, not because the service needs them.

Docker was not running on this machine; `systemctl --user start
docker-desktop.service` started it, which needs no elevation and is reversible
with the matching stop.

CI run 30688730458 closed the remaining gap: the four-way packaging matrix and
the container end-to-end check both passed on ubuntu 22.04 and 24.04 across
amd64 and native arm64.

Ticket 16 then closed that gap. With a test-only polkit rule granting
authorization inside the container, the check now drives the real
`DbusPrivilegedExecutor` through a real install and a real removal: apt runs,
dpkg confirms the package arrived and later left, the reported version matches
what dpkg holds, the health result is healthy, and the journal and rollback
record land on disk. A plan naming a prior version dpkg disagrees with is
refused and changes nothing.

That run immediately found a deadlock in shipped client code.
`execute_plan` watched `StepProgress` on a second thread, and that signal
iterator does not end when the call returns, so `std::thread::scope` waited
forever. Any real client — the CLI or the GUI — would have hung the moment it
reached a live daemon. Every test against fakes had passed. It now reads the
result from the outcome alone.

What remains untested is a failure after a real mutation: forcing the real
health check to fail would need a component built to fail it, and the catalog
has none. Rollback is covered against `FakeAptDriver` for all three outcomes,
but has never rolled back a real package.

Interactive polkit authentication is deliberately out of scope: collecting a
password is polkit's responsibility, and what this project has to prove is what
happens after polkit says yes.

Ticket 17 adds the durable installed-artifact mapping, strict rollback target
matching, metadata synchronization after rollback, and a container fixture for
an unhealthy update. The manager-daemon suite now passes with 42 unit tests and
6 D-Bus tests, the workspace fmt/check/test/clippy gates pass, the e2e client
builds, and the Debian package plus verifier pass.
The real mutation-failure container check is still blocked by the Docker API
permission failure described above.
The requested Issue #23 body update was attempted through the GitHub connector,
but the external mutation was rejected by the environment, so the issue remains
unchanged and open.

Ticket 17 fixed a rollback-target defect and proved the fix in a container. The
daemon used to derive `previous_artifact` from the artifact it was about to
install, so a failed update would have reinstalled the version that had just
failed and reported `Restored`. It now keeps a durable installed-artifact record
per component, written after each successful apply, and only uses it when its
version matches what dpkg reports and the file is still cached.

A rollback also verifies its own result now rather than trusting an exit code:
a removal is only counted once dpkg no longer reports the package, and a
reinstall only once dpkg reports the expected version.

Local `cargo fmt --all -- --check`, `cargo test --workspace` (21 suites, no
failures), `cargo clippy --workspace --all-targets -- -D warnings`, and the same
clippy run with the `dbus-client` feature all passed. `docker run` of the
container check passed on ubuntu-24.04/amd64: a deliberately unhealthy fixture
package installs, fails the real health check, and is really removed; and a
failed update over 0.0.9 ends with `dpkg-query` reporting 0.0.9 with the record
pointing at the 0.0.9 artifact.

CI run 30743821076 then ran the same change across all four supported
combinations. Every one reached the new rollback scenarios: a real failed update
ended with dpkg back at the previous version and the installed-artifact record
pointing at the previous artifact, on ubuntu 22.04 and 24.04, amd64 and native
arm64.

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
one. `docs/app-catalog-identity.md` records the model, the boundary each
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

Ticket 19 landed the Better App Chooser: `app-chooser-core` (MIME section
ranking, the `AppSelection` result model, the `mimeapps.list` editor and its
rollback records, and executable-mode refusals) and `app-chooser-gui` (the
reusable GPUI surface plus a standalone window for testing it). Neither crate
parses a `.desktop` file; both read ticket 18's records.

The three invariants this ticket adds. First, an Always Use changes exactly one
line of the user's `mimeapps.list`. The file is parsed into lines that are kept
verbatim — comments, unknown groups, repeated keys, CRLF endings, a missing
final newline — so everything except the one association is written back byte
for byte. Second, the rollback record is written and flushed before the file is
opened for writing, so a crash between the two leaves a record of a change that
never happened rather than a change with no record. Third, the executable mode
refuses by default: a Flatpak, Snap, AppImage, wrapper, D-Bus-activated, or
own-arguments entry is told why no path is offered instead of being handed the
program its `Exec` line happens to name.

MIME type relationships come from the installed `shared-mime-info` data files —
`aliases`, `subclasses`, and `globs2` — read only. There is no Better OS MIME
database, and an absent `shared-mime-info` degrades to "no known relationships"
rather than to a guessed hierarchy.

95 tests were added: 64 core unit tests, 8 fixture-file tests over six real
`mimeapps.list` shapes (hand edited with comments, groups in an unexpected order
with `[Default Applications]` opened twice, CRLF, no final newline, empty, and
comment-bearing), 13 GUI locale and layout-policy tests, plus the ranking,
selection, and executable-refusal coverage inside the core suites. The fixture
tests assert a single-line diff, that every unrelated association survives, and
byte equality after a rollback for every fixture.

Local `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo test
--workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo bench -p app-chooser-core` all passed. Ranking 5,000 synthetic records
takes 1.46 ms per pass, and it runs through `cx.background_spawn` rather than on
the render thread. An 8-second `ZED_HEADLESS=1` launch of `app-chooser-gui`
stayed alive in both Open With and Choose Executable mode.

Three things are known and not done. A `[Removed Associations]` line covering
the same type and application is reported as a warning rather than edited, so a
pre-existing removal can still override a new default; fixing it silently would
mean touching a second line the user wrote. Usage history is currently inferred
from the associations file, because no shared favorites or history model exists
yet — Issue #4 defers how that is shared with Better Launcher. And the chooser
handles one selected file; multi-file selection is out of scope for this ticket.

Ticket 22 replaced Better Monitor's mock `Sample` with typed metric and
capability contracts and added `monitor-collectors-linux`. A metric now carries
its unit, semantic type, source, support state, and sampling behaviour, and an
observation keeps unknown, unsupported, permission denied, stale, and a measured
zero apart. Six collectors read `/proc` and `/sys` directly — CPU, memory, PSI,
processes, storage throughput, and network interfaces — with no command
execution and no CLI parsing anywhere.

Every read goes through a `Roots` seam, so the tests drive the production path
against 597 fixture files: two `/proc` and `/sys` snapshots captured from this
machine three seconds apart, hand-authored synthetic pairs with exact expected
deltas, plus truncated, malformed, and no-PSI trees. 157 tests pass — 26 in
`monitor-core`, 117 collector unit tests, and 14 integration tests, two of which
run against the live host.

Four semantic traps the fixtures caught, all documented in
`docs/monitor-collector-sources.md`: `/proc/net/dev` pads interface names into a
fixed column so `tailscale0:` has no space before its first counter; `/proc/stat`
already folds guest time into `user`, so summing the ten columns double counts;
`/proc/vmstat`'s `pgpgin` counts kibibytes while `pswpin` in the same file counts
pages; and the capture machine is AMD, where `k10temp` publishes only `Tctl`, so
per-core temperature is genuinely unsupported rather than zero.

Measured overhead on this machine with 1,359 tasks, 100 rounds in release: mean
10.198 ms per full round, worst 12.209 ms, 99.0% CPU-bound. The process table is
9.178 ms of that; everything else together is about 1 ms. The 10,000-process
scenario from issue #16 has not been measured.

Gates run for ticket 22 were crate-scoped to avoid rebuilding GPUI in the
worktree: `cargo fmt --all -- --check`, `cargo check -p monitor-core -p
monitor-collectors-linux`, `cargo test -p monitor-core -p
monitor-collectors-linux`, `cargo clippy -p monitor-core -p
monitor-collectors-linux --all-targets -- -D warnings`, and `cargo check -p
monitor-gui`, all passing. `monitor-gui` was updated only far enough to speak
the new contract and still renders a fixed demonstration round. The full
workspace gate has not run on this branch and belongs on the main checkout after
merge. That full workspace gate ran on `main` immediately after this merge.

Ticket 23 is done. Better Monitor is no longer observation-only: the window
samples the real collectors on a background task once a second, and the
Overview, Apps, and Processes pages render what they produce. Two new crates
carry the work that must not live in GPUI — `monitor-views` for grouping and
the table, apps, and overview models, and `monitor-actions-linux` as the only
caller of `kill(2)` and `setpriority(2)` in the project. `monitor-core` gained
a typed action contract whose refusals are data a button renders from rather
than errors reported after the fact.

The grouping engine merges processes only where the system recorded a
relationship, and never because two executables share a name; that rule has its
own test. Each group carries the evidence that produced it and the confidence
of that evidence, and the precedence is configuration with Issue #16's order as
the default, because the final order still needs an ADR.

Gates that actually ran on this branch: `cargo fmt --all -- --check`,
`cargo check --workspace`, `cargo test --workspace` (626 tests, all passing; 513 on `main` before this ticket),
`cargo clippy --workspace --all-targets -- -D warnings`, an 8-second
`ZED_HEADLESS=1` launch of `monitor-gui` that stayed alive with no output, and
`cargo bench -p monitor-views` at 100, 1,000, and 10,000 processes. The
collector overhead measurement was re-run because adding the per-process
`/proc/[pid]/io` read made a round more expensive; the new number is published
in `docs/monitor-collector-sources.md` rather than left at the old one.

What ticket 23 did not close, and should not be assumed done: the narrow
polkit-reviewed boundary for cross-user and elevated process actions is not
built — those actions are refused with the owner named and no privileged helper
is reached for. Rendered frame time and dropped frames are not measured; only
the model-side cost is. Application grouping at 10,000 processes costs about
one frame on the interface thread. Keyboard-only operation is not verified.
"Open location" and "copy diagnostic details" from the ticket's deliverables
are not implemented.

### Ticket 25 — Better Awake Phase 1

Six new crates are on branch `ticket-25`: `awake-core`, `awake-ipc`,
`awake-store`, `awake-service`, `awake-tray`, and `awake-gui`. Nothing outside
them changed except the workspace member list, this file, and the ticket.

The service owns the inhibitor and the tray is a pure client, so a session
survives the tray being restarted — proved over a private session bus rather
than asserted. The local protocol is JSON documents over a session-bus
interface `org.betteros.Awake1`, the same shape ADR 0007 chose for the
privileged daemon; the ticket records why, because Issue #13 defers the final
choice to an ADR.

`ksni` was evaluated but not adopted, and the evaluation is incomplete on
purpose: this environment has no network access (crates.io answered HTTP 403),
and `ksni` is in neither the lockfile nor the local cargo cache, so its license
and maintenance could not be verified. The StatusNotifierItem and dbusmenu
surfaces are therefore written directly on zbus — about 300 lines that a crate
would replace without touching the menu model or the wording table. The ADR
still needs a network-capable environment.

Gates were run crate-scoped, because a workspace-wide run rebuilds the GPUI
world for each command. `cargo fmt --all -- --check` passed across the whole
workspace; check, test, and clippy `-D warnings` passed for the five non-GPUI
crates (142 tests, 11 of them on a private session bus); `cargo check -p
awake-gui` passed and the built binary stayed alive for 8 seconds under
`ZED_HEADLESS=1`. The full workspace gates have not been run and should run
downstream after merge.

Not done in Phase 1 and named in the ticket: the battery provider behind the
threshold, the rule engine the `自動規則` row disables itself for, packaging
(desktop entry, systemd user unit, Better Manager manifest), icon artwork for
the six states, and idle CPU/memory measurement for the two processes.
### Ticket 31 — Direct Removal storage crates

Ticket 31 added the three storage crates on branch `ticket-31`, not merged.
`storage-core` is the whole decision: normalized device identity, the Direct
Removal and Performance policies, six distinct states, the evidence model, the
per-device preference model, and restore-default plans, with no D-Bus, UDisks2,
`/proc`, or GPUI anywhere in it. The invariant everything else leans on is that
`ReadyToUnplug` cannot be built without a `ReadinessProof`, a proof cannot be
built without positive evidence, and the proof type has no `Deserialize`, so no
client can hand the service a green light it never earned.

Identity combines every stable identifier the platform reported — WWN, serial,
partition UUID, filesystem UUID — and falls back to vendor, model, and the
`by-path` port chain, which is honestly labelled weak. A device known only by
`/dev/sdb1` is volatile and can never hold a preference. Two connected devices
that report the same identity are both marked ambiguous and neither inherits the
stored preference.

`storage-platform` holds UDisks2 over zbus with real interface definitions,
`syncfs` on a mount for the flush, `BLKFLSBUF` reported honestly as unsupported
without privilege, per-backing-device writeback where debugfs allows it and the
machine-wide `/proc/meminfo` figure as a heuristic fallback, and a
`/proc/<pid>/fd` writer scan that counts what it could not inspect rather than
implying it saw everything. Every one of those is a trait with a fake behind the
`test-support` feature.

`storage-service` owns the session-long state and publishes it over
`org.betteros.Storage1` on the **session** bus — unprivileged, no polkit action,
because everything it does is something the logged-in user could do directly.
`docs/storage-safety-signals.md` records which signals are authoritative, which
are heuristic, which are unavailable, and exactly what would need privilege;
issue #5 defers that boundary to an ADR and nothing here pre-empts it.

129 tests pass across the three crates, plus three `#[ignore]`d live checks. The
probe binary run against this machine found 15 block devices, no external
hot-pluggable device connected, writeback available only as the machine-wide
figure, and 365 processes the writer scan could not inspect on `/` — which is
the unprivileged picture the safety document describes rather than a limitation
discovered later. Gates were crate-scoped to avoid rebuilding GPUI in the
worktree; the workspace gate belongs on the main checkout after merge. Real
device throughput across exFAT, NTFS, and ext4 on flash, SSD, and spinning
external disks needs hardware and is recorded as a follow-up in the ticket, not
approximated by the synthetic benchmarks.
