# Better OS

Better OS is a modular performance-improvement layer for Zorin OS and Ubuntu.
It replaces, enhances, or diagnoses desktop and system components one workload
at a time. It is not a Linux distribution fork.

## Current scaffold

- `better-core` validates versioned component manifests.
- `manager-core` creates non-privileged dry-run plans.
- `manager-cli` lists, validates, reports status, and prints plans for example
  components.
- `monitor-core` defines samples, incidents, inventory, and redacted exports.
- `better-ui`, `manager-gui`, and `monitor-gui` provide the GPUI application
  boundary and mock screens.

## Development

```bash
RUST_FONTCONFIG_DLOPEN=1 PATH="$HOME/.cargo/bin:$PATH" cargo fmt --all -- --check
RUST_FONTCONFIG_DLOPEN=1 PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace
RUST_FONTCONFIG_DLOPEN=1 PATH="$HOME/.cargo/bin:$PATH" cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Run the CLI from the repository root:

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo run -p manager-cli -- validate
PATH="$HOME/.cargo/bin:$PATH" cargo run -p manager-cli -- plan better-monitor
```

Read [`AGENTS.md`](AGENTS.md) before changing the project. The architecture
and current handoff state live in [`ENG.md`](ENG.md) and
[`delivery-status.md`](delivery-status.md).
