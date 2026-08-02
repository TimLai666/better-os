# Delivery Status

## Current Phase

Better Manager applies real component transactions, including rollback after a
real mutation failure

## Stage Objective

Turn Better Manager from a planning-only tool into one that actually installs,
updates, removes, and rolls back first-party components, without weakening the
boundary that keeps privileged mutation out of the GUI and CLI.

## Active Workstreams

- Shared manifest schema and validation
- Manager dry-run planning and CLI
- Better Manager GPUI shell, shared mock lifecycle, persistence, and acceptance coverage
- Manifest-declared presentation, platform boundary, and dark-first appearance
- Monitor observation contracts
- Release packaging contract, clean-install verification, and license notices
- Privileged daemon IPC contract, real artifact download, and APT execution

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

None outstanding. The next change should be scoped to a new ticket or to one of
the recorded follow-ups: package signing, the public APT repository, a repair
action for a transaction interrupted mid-flight, or aligning the declared Rust
baseline with what the lockfile actually requires.

## Next Ticket

None — tickets 09 through 17 are complete.

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
- decision: make the installed artifact record authoritative for rollback target selection
  rationale: a transaction rollback record describes one prior transaction, while
  only a version-matched component record can prove which cached artifact produced
  the version dpkg currently reports; the new artifact must never be a fallback
  for an old version
  timestamp: 2026-08-02
  impacted_ticket_ids: [17]

## Source Links

- [Issue #1](https://github.com/TimLai666/better-os/issues/1)
- [Issue #8](https://github.com/TimLai666/better-os/issues/8)
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

The four-way CI matrix has not run for this change yet.
