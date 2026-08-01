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
                    │   manager-core     │◄──── manager-cli / manager-gui
                    └─────────┬──────────┘
                              │ validated plans + deterministic mock lifecycle
                    ┌─────────▼──────────┐
                    │ future privileged │
                    │ execution boundary│
                    └────────────────────┘

             manager-cli / manager-gui ─────► manager-store
                                               versioned local JSON mock state

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
3. Resolve declared artifact download and disk requirements, reject an
   insufficient mock profile, and produce a dry-run plan that requires a
   current state revision at approval. Missing catalog or profile data stays
   explicitly unavailable rather than becoming a guessed estimate.
4. Advance the approved plan through deterministic mock download, install,
   settings, and health stages, persisting each state without host mutation.
5. Record failure evidence and a mock restore outcome when verification fails.
6. In a future privileged boundary, execute validated operations through local
   APT and replace the mock executor without changing the GUI or CLI contract.

Only steps 1–5 exist in this scaffold. Lifecycle descriptors remain data and
are never interpreted as commands.

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
still required when a desktop binary starts. These `*-dev` packages are
build-time dependencies only. The first release format is a `.deb`; its
metadata must declare the runtime libraries so `apt install ./<package>.deb`
works on a clean supported desktop without manual development-package setup.
See [Release Packaging Specification](release-packaging.md) for the release
verification contract.
