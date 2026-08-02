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
                    └────┬───────────┬───┘
                         │           │ validated plans + deterministic
                         │           │ mock lifecycle
      host capabilities  │           │
                    ┌────▼───────────▼───┐
                    │  manager-platform  │
                    │ capability/download│
                    │ package/privileged │
                    └─────────┬──────────┘
                              │ every shipped implementation is a mock
                    ┌─────────▼──────────┐
                    │ future privileged  │
                    │ execution boundary │
                    └────────────────────┘

             manager-cli / manager-gui ─────► manager-store
                                               versioned local JSON mock state

                    ┌────────────────────┐
                    │    monitor-core    │◄──── monitor-gui
                    └─────────┬──────────┘
                              │ samples, events, exports
                         local-first store

                    monitor-gui ──► monitor-ipc ──► org.betteros.Monitor1
                                                      │ distinct Polkit action
                                                      ▼
                                              bounded SMBIOS memory report

                    better-ui ────────► manager-gui / monitor-gui
```

## Trust boundaries

Manifest files, release metadata, collector output, and future remote data are
untrusted. Parsing and validation happen before planning. A manifest also
declares its own presentation and restart metadata, which is validated and
restricted to a closed icon set before any screen uses it. The GUI never
performs a privileged change itself; it asks the privileged service, which
authorizes the caller through polkit and revalidates the plan against the host
before acting. The service accepts an explicit, validated transaction plan and
returns an execution log, health result, and rollback record. An executor
without an authorized connection to it refuses every request.

## Manager transaction shape

1. Resolve a component and its manifest dependencies.
2. Verify target compatibility and artifact metadata against the profile
   `manager-platform` reports.
3. Resolve declared artifact download and disk requirements, reject an
   insufficient mock profile, and produce a dry-run plan that requires a
   current state revision at approval. Missing catalog or profile data stays
   explicitly unavailable rather than becoming a guessed estimate.
4. Advance the approved plan through download, install, settings, and health
   stages, persisting each state as it goes. A driver decides what each stage
   actually did: a simulation scripts it, a real transaction observes it.
5. Record failure evidence and the restore outcome when verification fails.
   Carry the declared replacements, enhancements, and the widest restart
   requirement into the plan so a reviewer sees them before approving.
6. For a real transaction, fetch and verify every artifact, hand each to the
   privileged service over a file descriptor, and give it the whole plan. The
   service revalidates independently, applies through local APT, health-checks
   what it applied, and rolls back what it can. The executor was replaced
   without changing the GUI or CLI contract: the same lifecycle stages, driven
   by a different driver.

Lifecycle descriptors remain data and are never interpreted as commands, on
either side of the privileged boundary.

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


### Monitor privileged-read boundary

Memory usage and all normal collectors remain unprivileged. The only privileged
monitor operation is an explicit, user-triggered SMBIOS memory inventory. The
root daemon exposes it on `org.betteros.Monitor1`, separate from package
mutation, and checks `org.betteros.monitor.read-memory-devices` through Polkit.
Raw firmware data never crosses D-Bus; `monitor-ipc` excludes serial numbers,
asset tags, and paths and validates every string and collection bound.
