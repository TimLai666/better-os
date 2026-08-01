# Engineering Plan: Better OS

## Architecture

```text
component manifests ──> better-core validation ──> manager-core
                                                   plan + mock lifecycle state
                                                        ├── manager-cli ──┐
                                                        └── manager-gui ──┼── manager-store JSON
                                                                          └── versioned local state

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
3. `manager-store` loads a versioned JSON state file. A future schema is
   preserved, malformed current-schema input is backed up, and stale writers
   are rejected.
4. `manager-core` validates the state against the catalog, resolves declared
   download, release-note, and disk-space metadata, produces a deterministic
   dry-run plan, and owns every mock lifecycle transition. A plan is rejected
   when declared disk space exceeds the mock profile. No plan step can mutate
   the host in this stage. Its errors expose stable machine keys; presentation
   layers own localized user-facing wording.
5. The CLI and GUI both review the same plan, persist each mock stage, and can
   resume a valid active operation after restart.
6. Monitor collectors later emit samples into `monitor-core`; this stage only
   defines the collector, storage, incident, and export contracts.
7. Release packaging derives and verifies runtime dependencies from the final
   binaries for each supported target and architecture.
8. A clean desktop environment installs the `.deb` through local APT and runs
   the manager and monitor launch smoke tests.

## Test seams

- Manifest behavior is tested through public parse and catalog-validation APIs.
- Manager behavior uses mock state and asserts observable plans, stages,
  failure evidence, recovery results, and status results.
- `manager-store` is an adapter seam for versioned JSON reload, stale-write,
  corrupt-state, and restart-resume tests. It never decides a lifecycle state.
- Monitor behavior uses a fake collector and in-memory history.
- `manager-gui` uses the demo catalog and state to assert that its Update All
  path produces the same dry-run plan as `manager-core`, and tests both
  locales plus 100/125/150% long-label layout policy.
- GUI crates depend on the shared core crates; launch smoke coverage runs in an
  environment with a display backend.

## Test matrix

| Area | Required proof |
| --- | --- |
| Manifest | valid input, missing fields, schema version, cycles, conflicts |
| Manager | list, status, declared disk-space preflight, release-note propagation, install/update/enable/disable/verify/restore mock lifecycle, no host mutation |
| Manager store | schema preservation, atomic JSON writes, stale writers, restart resume |
| Monitor | sample storage, incident creation, export redaction boundary |
| GUI | workspace build and launch smoke test on a Linux desktop |
| Release package | no `*-dev` in `Depends`, clean APT install, dynamic-library check, manager and monitor launch |

## Migration plan

No database or system migration is part of the initial scaffold. Better
Manager uses a versioned local JSON file only for mock lifecycle state. A
database remains unnecessary until concurrent multi-process history, query, or
retention requirements are explicit. Local monitor history must remain
replaceable until its storage decision is explicit.

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
