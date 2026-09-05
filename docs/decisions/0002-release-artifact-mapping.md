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
- The published `v0.1.0` checksums are recorded in the two release-eligible
  manifests. The approved maintainer and root project license are recorded in
  ADR 0003 and the package build script.

## v0.1.0 verification

The first release is published at
<https://github.com/TimLai666/better-os/releases/tag/v0.1.0> from merge commit
`3a6d98b73b838c5a2c0d94404ae9313844009e56`. Post-merge CI run
<https://github.com/TimLai666/better-os/actions/runs/30650287246> passed the Rust
checks and all four target/architecture package jobs. The public release was
downloaded again and all eight `.deb.sha256` sidecars verified their matching
`.deb` assets before the manifest checksums were committed.

## v0.2.0 verification

The second release is published at
<https://github.com/TimLai666/better-os/releases/tag/v0.2.0> from merge commit
`96b46f11e814bd4088f630c9c19727e45af9132f`. Post-merge CI run
<https://github.com/TimLai666/better-os/actions/runs/33389237001> passed the Rust
checks and all four target/architecture package jobs. The release carries all
eight packages — the two from v0.1.0 plus `better-manager-daemon`,
`better-launcher`, `better-files`, `better-touchpad`, `better-awake`, and
`better-storage` — as 32 `.deb` assets and 32 `.deb.sha256` sidecars. The naming
contract above kept every filename distinct across the four jobs. The public
release was downloaded again and all 32 sidecars verified their matching `.deb`
assets before the manifest checksums were committed.

`better-monitor` moved to 0.2.0 because its payload changed: v0.1.0 shipped the
window alone and the package now also carries the session service, the command
line, and a systemd user unit. A manifest checksum maps to one release asset, so
one version number may not name two payloads.

## v0.2.1 verification

The third release is published at
<https://github.com/TimLai666/better-os/releases/tag/v0.2.1> from merge commit
`8dc9c7eede37917d5af527a0d8df17e84213b48b`. Post-merge CI run
<https://github.com/TimLai666/better-os/actions/runs/33871736272> passed the Rust
checks and all four target/architecture package jobs. It is a patch release over
v0.2.0 and carries the same eight packages as 32 `.deb` assets and 32
`.deb.sha256` sidecars; the payload change is `better-touchpad`, which now ships
the `touchpad-adapter@betteros.org` GNOME Shell extension and the
`better-touchpad-gestured` service. The public release was downloaded again, all
32 sidecars verified their matching `.deb` assets, and each recorded checksum was
re-hashed from the downloaded package before the manifests were committed.

The bump also settled what a manifest checksum means across a release branch: the
values a manifest carries describe the *previous* release's assets, so the seven
shipped manifests went back to placeholders when the version moved and took real
values again only after v0.2.1 was public.

## v0.2.2 verification

The fourth release is published at
<https://github.com/TimLai666/better-os/releases/tag/v0.2.2> from merge commit
`b5f6e34edad24e199181ce131f4c8a5b490c7fbe`. Post-merge CI run
<https://github.com/TimLai666/better-os/actions/runs/33942768617> passed the Rust
gate, the installer job, and all four target/architecture package jobs. It
carries the same eight packages as 32 `.deb` assets and 32 `.deb.sha256`
sidecars; the payload change is `better-manager`, which now refreshes its
component catalog from the published manifests. The public release was
downloaded again, all 32 sidecars verified their matching `.deb` assets, the
downloaded bytes were byte-identical to the CI artifacts, and each recorded
checksum was re-hashed from the downloaded package before the manifests were
committed.

This release is also where the placeholder rule above was verified from the
other side. `install.sh` resolved `v0.2.2` from the public API, and a build of
merge commit `b5f6e34` — whose compiled-in catalog carries this release's
placeholder checksums — refreshed the seven manifests from `main`, planned
`better-monitor` 0.2.2, and verified the real published `.deb` against the
fetched checksum. The naming contract is what let it do so: the manifest names
one unambiguous asset, and the fetched checksum described that exact file.

## Deferred

Release automation, package signing, public APT repositories, and release
channels remain outside this decision.
