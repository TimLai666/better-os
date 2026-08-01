# Delivery Status

## Current Phase

Better Manager Issue #8 gap closure handed off for branch review

## Stage Objective

Keep the component contract, non-privileged manager planning path, and the
Issue #8 Better Manager UI verifiable before adding real system integration.

## Active Workstreams

- Shared manifest schema and validation
- Manager dry-run planning and CLI
- Better Manager GPUI shell, shared mock lifecycle, persistence, and acceptance coverage
- Manifest-declared presentation, platform boundary, and dark-first appearance
- Monitor observation contracts
- Release packaging contract and clean-install verification plan

## Milestones

| id | target | owner | status | verification_signal |
| --- | --- | --- | --- | --- |
| M1 | workspace and shared contracts | agent | done | `cargo test -p better-core` |
| M2 | manager dry-run path | agent | done | CLI list/status/plan output |
| M3 | monitor and GUI shells | agent | done | `cargo build --workspace`; both GUI binaries stayed alive for 8 seconds in a Wayland session |
| M4 | docs and CI | agent | done | workflow file and docs review |
| M5 | release packaging contract | agent | done | `docs/release-packaging.md` and ticket 06 acceptance criteria |
| M6 | target-compatible `.deb` packaging | agent | done | GitHub Actions Ubuntu 22.04/24.04 amd64 and native arm64 matrix build, dependency metadata check, checksum verification |
| M7 | Better Manager Issue #8 functional acceptance | agent | done | Chefer AppCipe passed fmt, workspace check/test, clippy, CLI lifecycle smoke, and GUI headless smoke |
| M8 | Issue #8 remaining gap closure | agent | done | manifest presentation and restart metadata, `manager-platform`, manifest-driven GUI, dark-first appearance, ADRs 0002–0004 |

## Current Blockers

Issue #8 has no active implementation blocker. Every acceptance criterion in
the issue is now met, and the five remaining gaps from the branch audit are
closed under ticket 08. Ticket 06 still needs public
target-specific release assets, manifest checksum verification, and the
approved maintainer contact before publishing. The current matrix produces four
same-named package assets while each manifest has one artifact checksum, so the
asset naming and manifest mapping must be decided before a release can be
created.

The declared Rust 1.85 baseline is also incompatible with the current lockfile:
an isolated Rust 1.85 build stops before compilation because dependencies now
require up to Rust 1.92. The GitHub workflow uses stable Rust. The Issue #8
Chefer AppCipe passed with Rust 1.97. The supported toolchain policy still
needs alignment.

## Next Verifiable Output

Publish uniquely named target-specific release assets, then derive and verify
the manifest checksums from those public assets.

## Next Ticket

06 — Release packaging

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
- decision: persist Better Manager's mock lifecycle in versioned local JSON and
  show disk or release metadata only when its catalog declares it
  rationale: the user chose JSON over a database; explicit unavailable values
  preserve truthful review screens without inventing release data
  timestamp: 2026-07-31
  impacted_ticket_ids: [07]
- decision: open Better Manager dark by default and keep light and
  system-follow as explicit stored choices
  rationale: Issue #8 states a dark-first UI direction, and the shipped
  light-only slice contradicted it; see ADR 0002
  timestamp: 2026-08-01
  impacted_ticket_ids: [08]
- decision: move every host-facing interface into `manager-platform` and keep
  each shipped implementation a mock that refuses to apply a package change
  rationale: Issue #8 lists the crate boundary and requires the privileged
  executor to stay an interface until its security design is approved; a mock
  that returned success would claim the host changed; see ADR 0003
  timestamp: 2026-08-01
  impacted_ticket_ids: [08]
- decision: declare component summary, icon, and restart scope in the manifest
  with a closed icon set, and present untranslated components from those values
  rationale: the GUI matched three hardcoded component IDs and dropped every
  other component; manifests are untrusted, so the icon set stays closed; see
  ADR 0004
  timestamp: 2026-08-01
  impacted_ticket_ids: [08]

## Source Links

- [Issue #1](https://github.com/TimLai666/better-os/issues/1)
- [Issue #8](https://github.com/TimLai666/better-os/issues/8)
- [ENG.md](ENG.md)
- [Architecture](docs/architecture.md)
- [Release packaging](docs/release-packaging.md)
- [Pull request #9](https://github.com/TimLai666/better-os/pull/9)
- [Pull request #11](https://github.com/TimLai666/better-os/pull/11)
- [CI run 30616628027](https://github.com/TimLai666/better-os/actions/runs/30616628027)
- [CI run 30625827340](https://github.com/TimLai666/better-os/actions/runs/30625827340)
- [Main CI run 30631266909](https://github.com/TimLai666/better-os/actions/runs/30631266909)
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
The post-merge `main` CI run passed Rust checks and all four package jobs.
The package payload also started both GUI binaries for 12 seconds in the host's
Zorin OS 18.1 GNOME Wayland session with `ZED_HEADLESS` unset. Docker's Xvfb and
host-socket tests still failed because they did not provide a usable compositor,
but the direct host session passed. No public release asset exists yet, and
manifest checksum values and maintainer approval remain open. The four-platform
asset naming and manifest mapping decision is also open.
PR #9 is merged into `main` at `fb3520f`, and the feature branch has been
deleted from both the local checkout and GitHub.

Issue #8 now keeps the CLI and GPUI on the shared `manager-core` lifecycle API,
persists mock state through `manager-store`, and localizes the GUI at runtime.
The final disposable Chefer AppCipe passed `cargo fmt --all -- --check`,
`cargo check --workspace --offline --quiet`, `cargo test --workspace --offline
--quiet`, workspace clippy with `-D warnings`, CLI lifecycle smoke, and an
8-second `ZED_HEADLESS=1` GUI launch smoke. No privileged command or real
package mutation ran.
The temporary `better-manager-gpui-complete/` directory was moved to the
desktop trash after the destination file set was verified.

A branch audit against the Issue #8 text then found five gaps outside the
acceptance-criteria list: no dark theme, no `manager-platform` crate,
`replaces`/`enhances` never surfaced, component icon and purpose hardcoded to
three component IDs, and a `RestartRequirement` with only `NotDeclared`. All
five are closed under ticket 08, with ADRs 0002, 0003, and 0004 recording the
decisions Issue #8 asked to be written down rather than made silently. The
hardcoded-ID map also silently dropped any component outside that list from
the GUI; presentation is now manifest-driven, so it does not.
