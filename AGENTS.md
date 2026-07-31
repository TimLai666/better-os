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

- Keep privileged system mutation outside GUI and CLI code. This issue only
  permits planning and mock/in-memory execution.
- Keep manager CLI and GUI on the same `manager-core` planning API.
- Use Rust for first-party production code. Use Go only after recording a
  concrete reason in an ADR. Do not add C, C++, Python, JavaScript, Electron,
  Tauri, or GTK application code.
- Use GPUI with `gpui-component` for first-party desktop GUIs. `better-ui` owns
  shared presentation primitives.
- Treat manifests as untrusted input: validate schema, targets, artifacts,
  dependencies, conflicts, and lifecycle metadata before planning.
- Do not add a project license, public APT repository, signing implementation,
  privileged daemon IPC protocol, or automatic optimizer without an explicit
  decision.
- Every behavior change needs tests. Run formatting, linting, workspace checks,
  and tests before handoff.

## Handoff

Before handoff, update `delivery-status.md`, keep the next ticket accurate,
record active blockers, and state which checks actually ran. Do not claim a
GUI or dependency compiles when the relevant command was not executed.

## Follow-ups

- Decide the root project license before publishing distributable artifacts.
- Set an approved maintainer contact in Debian control metadata before
  publishing `.deb` artifacts.
- Build each supported Ubuntu release in a compatible base environment. The
  current Zorin 18 host produces `libc6 (>= 2.39)` and must not supply a 22.04
  release artifact.
- Update GitHub Actions dependencies after the Node.js 20 deprecation warning
  on `actions/checkout` and `actions/upload-artifact` is addressed.
- Decide the privileged daemon IPC protocol before implementing real system
  installation or rollback.
- Review the license implications of every copyleft dependency before release.
- Add real Linux collectors and benchmark runners only after the mock contracts
  are stable.
