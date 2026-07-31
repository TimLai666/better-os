# Better OS Architecture

## Boundaries

```text
                    ┌────────────────────┐
                    │ component manifests│
                    └─────────┬──────────┘
                              │ parse + validate
                    ┌─────────▼──────────┐
                    │    better-core     │
                    └─────────┬──────────┘
                              │ domain contracts
                    ┌─────────▼──────────┐
                    │   manager-core     │◄──── manager-cli
                    └─────────┬──────────┘
                              │ planning only
                    ┌─────────▼──────────┐
                    │ future privileged │
                    │ execution boundary│
                    └────────────────────┘

                    ┌────────────────────┐
                    │    monitor-core    │◄──── monitor-gui
                    └─────────┬──────────┘
                              │ samples, events, exports
                         local-first store

                    better-ui ────────► manager-gui / monitor-gui
```

## Trust boundaries

Manifest files, release metadata, collector output, and future remote data are
untrusted. Parsing and validation happen before planning. The GUI never receives
a privileged executor. A future privileged daemon must accept an explicit,
validated transaction plan and return an execution log, health result, and
rollback record.

## Manager transaction shape

1. Resolve a component and its manifest dependencies.
2. Verify target compatibility and artifact metadata.
3. Produce a dry-run plan.
4. In a future privileged boundary, execute the plan through local APT.
5. Run health checks and record rollback information.

Only steps 1–3 exist in this scaffold.

## Monitor observation layers

- Continuous: low-cost CPU, memory, PSI, and inventory samples.
- Periodic: audits that refresh component and system state.
- Event-triggered: deeper profiling around a user-marked incident or detected
  regression.

The monitor stores locally by default. Export is explicit and redacts sensitive
values before data leaves the local process.

## Linux GUI prerequisites

The GPUI desktop binaries use the Linux X11 and Wayland backends. A development
machine needs `libfontconfig1-dev`, `libxcb1-dev`, `libxkbcommon-dev`, and
`libxkbcommon-x11-dev` to link the binaries. CI installs these packages before
the workspace checks. `RUST_FONTCONFIG_DLOPEN=1` avoids requiring the
fontconfig development metadata at compile time, but the runtime library is
still required when a desktop binary starts.
