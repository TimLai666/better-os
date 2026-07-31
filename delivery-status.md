# Delivery Status

## Current Phase

Initial monorepo foundation

## Stage Objective

Make the component contract and non-privileged manager planning path verifiable
before adding real system integration.

## Active Workstreams

- Shared manifest schema and validation
- Manager dry-run planning and CLI
- Monitor observation contracts

## Milestones

| id | target | owner | status | verification_signal |
| --- | --- | --- | --- | --- |
| M1 | workspace and shared contracts | agent | done | `cargo test -p better-core` |
| M2 | manager dry-run path | agent | done | CLI list/status/plan output |
| M3 | monitor and GUI shells | agent | blocked | `cargo check --workspace`; GUI link/runtime needs Linux desktop libraries |
| M4 | docs and CI | agent | done | workflow file and docs review |

## Current Blockers

- Local GUI test binaries cannot link until `libxcb1-dev`, `libxkbcommon-dev`,
  and `libxkbcommon-x11-dev` are installed. CI installs them automatically.

## Next Verifiable Output

Run the GUI smoke test in CI or on a Linux desktop with the documented GPUI
development libraries.

## Next Ticket

04 — 使用者可以開啟 manager 與 monitor 的 GPUI mock shell

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

## Source Links

- [Issue #1](https://github.com/TimLai666/better-os/issues/1)
- [ENG.md](ENG.md)
- [Architecture](docs/architecture.md)
- [Tickets](docs/tickets/)

## Handoff Notes

The checkout started with only `README.md`. Rust is available through
`/home/tim/.cargo/bin`, but is not on the default shell `PATH`.
