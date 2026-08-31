# 36 — Package the new component suite

**Epic:** Release surface for issues #2, #3, #5, #6, #13 (and #16's service)
**User Story:** Better Manager can actually install, verify, and remove every
component the suite now implements, because each one builds into a real `.deb`
whose checksum its manifest can name.
**Blocked by:** 18–35 (all merged)
**Status:** todo

## Goal

`packaging/build-deb.sh` builds only the original three packages. Every new
component's manifest carries placeholder artifact checksums, so none is
release-eligible, and Issue #13's "Better Manager can install, verify, disable,
and remove the component cleanly" cannot be verified. Close that gap.

## What it delivers

- `build-deb.sh` (and `verify-deb.sh`) extended to build and verify packages
  for: `better-launcher` (overlay binary + desktop entry), `better-files`
  (binary + desktop entry), `better-touchpad` (binary + desktop entry + safe-
  mode desktop entry), `better-awake` (service + tray + gui binaries, systemd
  user unit, desktop entries), `better-monitor` (gui + service + cli, systemd
  user unit), `better-storage` (service + doctor, systemd user unit), and the
  app-chooser binary where a component needs it. Follow the existing packaging
  conventions: runtime dependency metadata, license notices payload, checksum
  sidecars.
- Existing packaging data under `packaging/awake/` and `packaging/launcher/`
  is used, not duplicated; missing units/desktop entries are added there.
- Component manifests updated so declared artifacts map to the produced
  package filenames (checksums stay per-build; the placeholder pattern the
  repo already uses for pre-release manifests is acceptable if that is the
  established convention — follow ADR 0002's mapping rules).
- CI workflow matrix extended so the new packages build on the same
  release/architecture matrix; the package-verifier covers them.
- An install/remove smoke: at minimum `verify-deb.sh` assertions per package
  (control fields, payload, notices, no `*-dev` deps); a container
  apt-install/remove check for one new package if the existing e2e harness
  can carry it without new infrastructure.

## Out of scope

- Publishing a GitHub release.
- Package signing (deferred by explicit decision).
- Chefer AppCipe changes beyond what the container check needs.

## Verification

`cargo fmt --all -- --check`, `cargo check --workspace`,
`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
warnings`, `packaging/build-deb.sh` and `packaging/verify-deb.sh` locally for
every package, and the workflow file updated coherently.
