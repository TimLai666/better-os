# Delivery Status

## Current Phase

Initial monorepo foundation and release packaging contract

## Stage Objective

Make the component contract and non-privileged manager planning path verifiable
before adding real system integration.

## Active Workstreams

- Shared manifest schema and validation
- Manager dry-run planning and CLI
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
| M6 | target-compatible `.deb` packaging | agent | done | GitHub Actions Ubuntu 22.04/24.04 matrix build, dependency metadata check, checksum verification |

## Current Blockers

No active blocker remains for the GUI smoke test or the target-compatible
Ubuntu package matrix. Ticket 06 still needs arm64 packaging, clean supported
system install and launch records, manifest checksum verification, and an
approved maintainer contact before publishing.

## Next Verifiable Output

Download the matrix artifacts from CI, repeat the clean Ubuntu 22.04 and 24.04
install and launch checks, then verify the manifest checksum and complete the
arm64 packaging path.

## Next Ticket

06 — 使用者可以在乾淨的支援系統安裝並啟動 release package

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

## Source Links

- [Issue #1](https://github.com/TimLai666/better-os/issues/1)
- [ENG.md](ENG.md)
- [Architecture](docs/architecture.md)
- [Release packaging](docs/release-packaging.md)
- [Pull request #9](https://github.com/TimLai666/better-os/pull/9)
- [CI run 30616628027](https://github.com/TimLai666/better-os/actions/runs/30616628027)
- [Tickets](docs/tickets/)

## Handoff Notes

The checkout started with only `README.md`. Rust is available through
`/home/tim/.cargo/bin`, but is not on the default shell `PATH`.

The Ubuntu 22.04/24.04 package matrix passed in CI, including package build,
runtime dependency checks, checksums, and artifact upload. No public release
asset exists yet. Ticket 06 remains in progress until the clean-system, arm64,
manifest-checksum, and maintainer criteria pass.
PR #9 is merged into `main` at `fb3520f`, and the feature branch has been
deleted from both the local checkout and GitHub.
