# Engineering Plan: Better OS

## Architecture

```text
component manifests ──> better-core validation ──> manager-core planning
                                                        ├── manager-cli
                                                        └── manager-gui

collectors ──> monitor-core samples/incidents/export ──> monitor-gui

better-ui provides GPUI presentation primitives to both GUI crates.
Privileged execution is a future boundary and is not implemented here.

Release builds package the final GUI binaries as `.deb` assets. Package
metadata declares runtime dependencies; the development-only GPUI linker
packages stay in the build environment and are never a user installation
prerequisite.
```

## Data flow

1. The CLI or GUI loads manifests from the catalog.
2. `better-core` parses and validates each manifest.
3. `manager-core` builds a dry-run transaction plan against an in-memory
   backend. No plan step can mutate the host in this stage.
4. Monitor collectors later emit samples into `monitor-core`; this stage only
   defines the collector, storage, incident, and export contracts.
5. Release packaging derives and verifies runtime dependencies from the final
   binaries for each supported target and architecture.
6. A clean desktop environment installs the `.deb` through local APT and runs
   the manager and monitor launch smoke tests.

## Test seams

- Manifest behavior is tested through public parse and catalog-validation APIs.
- Manager behavior uses an in-memory backend and asserts observable plans and
  status results.
- Monitor behavior uses a fake collector and in-memory history.
- GUI crates depend on the shared core crates; UI smoke coverage is deferred to
  an environment with a display backend.

## Test matrix

| Area | Required proof |
| --- | --- |
| Manifest | valid input, missing fields, schema version, cycles, conflicts |
| Manager | list, status, dry-run install/update, no host mutation |
| Monitor | sample storage, incident creation, export redaction boundary |
| GUI | workspace build and launch smoke test on a Linux desktop |
| Release package | no `*-dev` in `Depends`, clean APT install, dynamic-library check, manager and monitor launch |

## Migration plan

No database or system migration is part of the initial scaffold. Local monitor
history must remain replaceable until the storage decision is explicit.

## Hidden assumptions

- GitHub Release assets will later provide checksums and signatures, but the
  signature format is intentionally not chosen yet.
- Manifest lifecycle commands are data only. No component command is executed
  by the current manager.
- GPUI is pre-1.0 and may require the latest stable Rust and Linux display
  dependencies.
- The final runtime dependency list is target- and architecture-specific and
  must be taken from the packaged binaries rather than copied from CI's
  build-time package list.
