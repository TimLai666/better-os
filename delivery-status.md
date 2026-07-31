# Delivery Status

## Current Phase

Better Monitor live monitoring UI

## Stage Objective

Turn the existing monitor contracts and mock shell into a useful, non-privileged
GPUI system monitor while keeping unsupported Linux collectors explicit.

## Active Workstreams

- Better Monitor live CPU, memory, process, storage, and network presentation
- Better Monitor incident markers, short-term history, and coverage diagnostics
- Shared manifest schema and non-privileged manager planning
- Release packaging contract, clean-install verification, and license notices

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
| M9 | first functional Better Monitor UI | agent | in_review | PR #18 Rust checks, locked dependency notices, and Ubuntu package matrix |

## Current Blockers

No implementation blocker is known for the current Better Monitor slice. The PR
must still pass the complete Ubuntu 22.04/24.04 amd64 and arm64 package matrix,
and the rendered desktop UI still needs visual and interaction review in a real
GNOME Wayland session before merge.

## Next Verifiable Output

Review PR #18 on a real desktop session, verify navigation, charts, table
scrolling, resizing, light/dark themes, and one-second refresh behavior, then
record any visual defects as focused follow-up work under issue #16.

## Next Ticket

Issue #16 — Better Monitor production collectors, history service, and task actions

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
- decision: use `sysinfo` as the portable baseline dependency for the first live
  Better Monitor UI, then add Linux-specific collectors behind typed boundaries
  rationale: deliver useful CPU, memory, process, disk, and network data without
  pretending a portable API covers PSI, cgroups, GPU engines, SMART, or storage
  latency semantics
  timestamp: 2026-08-01
  impacted_ticket_ids: [16]
- decision: render unsupported monitor metrics as unavailable coverage states
  rather than numeric zeroes
  rationale: missing observation is evidence about the collector, not evidence
  that the measured activity is zero
  timestamp: 2026-08-01
  impacted_ticket_ids: [16]

## Source Links

- [Issue #1](https://github.com/TimLai666/better-os/issues/1)
- [Issue #16](https://github.com/TimLai666/better-os/issues/16)
- [Pull request #18](https://github.com/TimLai666/better-os/pull/18)
- [ENG.md](ENG.md)
- [Architecture](docs/architecture.md)
- [Release packaging](docs/release-packaging.md)
- [ADR 0002: Target-specific release assets](docs/decisions/0002-release-artifact-mapping.md)
- [ADR 0003: GPL-3.0-or-later root license](docs/decisions/0003-project-license.md)
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

PR #18 replaces the monitor mock screen with a live GPUI prototype backed by
`sysinfo` 0.37.2. It includes Overview, Apps, Processes, CPU, Memory, Storage,
Network, History, Incidents, and Diagnostics pages, but intentionally does not
claim PSI, cgroup application grouping, GPU, SMART, persistent history, or
privileged process actions. Closing the current GUI also ends collection because
the long-running monitor service has not been implemented yet.
