# Better OS

Better OS is a modular performance-improvement layer for Zorin OS and Ubuntu.
It replaces, enhances, or diagnoses desktop and system components one workload
at a time. It is not a Linux distribution fork.

## Current scaffold

- `better-core` validates versioned component manifests.
- `manager-core` creates deterministic non-privileged plans and mock lifecycle
  transitions for install, update, enable, disable, verify, and restore.
- `manager-store` persists only versioned local mock state with atomic writes,
  stale-writer protection, corrupt-state backup, and restart resume.
- `manager-platform` owns the system capability, download, package, and
  privileged-executor interfaces. Every shipped implementation is a mock, and
  no shipped code path applies a package change.
- `manager-cli` and `manager-gui` share that core API and never execute
  manifest lifecycle strings, APT, sudo, or shell commands.
- `monitor-core` defines samples, incidents, inventory, and redacted exports.
- `better-ui`, `manager-gui`, and `monitor-gui` provide the GPUI application
  boundary and mock screens. Better Manager is dark by default and also offers
  light and system appearances.

## Development

Run unreleased builds only in a disposable Chefer AppCipe, as required by
[`AGENTS.md`](AGENTS.md). The isolated verification command runs formatting,
workspace checks, tests, clippy, and CLI lifecycle smoke coverage without
installing a build or touching package state on the host.

Inside that disposable environment, use a scoped state path:

```bash
cargo run -p manager-cli -- --state-path /tmp/better-manager-state.json validate
cargo run -p manager-cli -- --state-path /tmp/better-manager-state.json run better-monitor install
cargo run -p manager-cli -- --state-path /tmp/better-manager-state.json status better-monitor
```

Read [`AGENTS.md`](AGENTS.md) before changing the project. The architecture
and current handoff state live in [`ENG.md`](ENG.md) and
[`delivery-status.md`](delivery-status.md). The manager screen behavior is
defined in [`docs/manager-ux-logic.md`](docs/manager-ux-logic.md). Accepted
decisions live in [`docs/decisions/`](docs/decisions/).
