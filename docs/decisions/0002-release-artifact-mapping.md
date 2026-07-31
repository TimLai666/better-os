# ADR 0002: Target-Specific Release Assets

## Status

Accepted on 2026-07-31.

## Context

The package matrix builds manager and monitor for Ubuntu 22.04 and 24.04 on
amd64 and arm64. The original package filenames were always
`better-manager.deb` and `better-monitor.deb`, so the four matrix jobs could
not be uploaded to one GitHub Release without collisions. A manifest with one
checksum also could not identify which target artifact it verified.

## Decision

Use one GitHub Release per project version, starting with the `v0.1.0` naming
contract. Each component publishes one `.deb` asset for every supported Ubuntu
release and architecture combination:

```text
<component>_<version>_ubuntu-<release>_<architecture>.deb
```

The matching `.deb.sha256` file is published as a checksum sidecar. Component
manifest schema version 2 replaces `artifact` with an `artifacts` list. Each
entry contains `release`, `architecture`, `url`, `release_asset`, and `sha256`.
The parser requires exactly one entry for every declared release/architecture
combination and rejects unsupported, duplicate, missing, malformed, or unsafe
variants.

The `release` field identifies the Ubuntu build environment. The artifact can
serve the compatible distributions listed in the manifest target matrix; the
schema does not create a separate package for each compatible distribution.

## Consequences

- The package filename itself identifies the build target and architecture.
- A manifest checksum maps to one unambiguous release asset.
- CI may use an architecture-specific job directory, but the asset target stays
  `ubuntu-22.04` or `ubuntu-24.04` for both architectures.
- The CI package verifier must select and validate target-specific filenames.
- Actual published checksums remain a release gate. The approved maintainer and
  root project license are recorded in ADR 0003 and the package build script.

## Deferred

Release automation, package signing, public APT repositories, and release
channels remain outside this decision.
