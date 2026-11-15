# ADR 0021: Release packages are built per Ubuntu release and architecture on native runners

## Status

Accepted on 2026-07-31 (ticket 06). Written as a record on 2026-11-15 from the
delivery status decision log.

## Decision

- Each supported Ubuntu release and architecture is built in its own compatible
  environment; arm64 runs on native GitHub-hosted arm64 runners.
- GPUI's `*-dev` packages exist only in the build environment. Each `.deb`
  declares the runtime libraries it was verified to need, so a person installs
  it through local APT with no compiler or development packages.

## Why

The manifests declare amd64 and arm64. Native runners avoid QEMU emulation and
keep one build environment per release and architecture. Package metadata is
the right place for the runtime dependency boundary.

## Options rejected

- Cross-compiling or emulating arm64 with QEMU: slower, and not the environment
  the package runs in.
- Asking users to install the development packages: makes a release depend on a
  build setup.

## Consequences

- A host newer than a target release must not produce that release's artifact
  (a Zorin 18 host links `libc6 (>= 2.39)`).
- Asset naming per target is ADR 0002.
