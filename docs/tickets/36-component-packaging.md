# 36 — Package the new component suite

**Epic:** Release surface for issues #2, #3, #5, #6, #13 (and #16's service)
**User Story:** Better Manager can actually install, verify, and remove every
component the suite now implements, because each one builds into a real `.deb`
whose checksum its manifest can name.
**Blocked by:** 18–35 (all merged)
**Status:** done

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

## What was built

Eight packages, all produced and verified locally on this amd64 host with
`--target local`:

| Package | Payload |
| --- | --- |
| `better-manager` | window |
| `better-manager-daemon` | privileged service, system unit, D-Bus and polkit files |
| `better-monitor` | window, `better-monitor-service`, `better-monitor-cli`, user unit |
| `better-launcher` | binary, desktop entry |
| `better-files` | binary, `io.betteros.Files.desktop` |
| `better-touchpad` | binary, desktop entry, safe-mode desktop entry |
| `better-awake` | service, tray, settings window, user unit, desktop entry, autostart entry |
| `better-storage` | service, doctor, user unit, session D-Bus activation file |

Packaging data added under `packaging/`: `monitor/better-monitor.service`,
`storage/better-storage.service`, `storage/org.betteros.Storage1.service`,
`files/io.betteros.Files.desktop`, `touchpad/better-touchpad.desktop`,
`touchpad/better-touchpad-safe-mode.desktop`. The existing `packaging/awake/`
and `packaging/launcher/` files are used unchanged.

Every package installs its systemd user unit and enables nothing. Enabling is
Better Manager's enable step, which is the split `better-manager-daemon`
already uses, and `verify-deb.sh` fails any package that ships a `.wants`
symlink.

## Decisions and gaps recorded rather than papered over

- **`better-monitor` now has two payloads under one version.** The manifest's
  checksums are real and belong to the published v0.1.0 asset, which is the
  window alone. This package is wider. Publishing it needs a version bump,
  which is a release decision and was not taken here.
- **The command line is installed as `better-monitor-cli`.** Its clap command
  name and its own error text say `better-monitor`, and the window already owns
  `/usr/bin/better-monitor` in a published package. Renaming either one is a
  crate change with user-visible consequences, so packaging chose the name that
  does not break what shipped and left the collision recorded.
- **No `better-touchpad` manifest exists**, so the package is real and Better
  Manager still cannot install, verify, or remove that component. Writing one
  means choosing health checks that actually exist and a benchmark budget
  somebody stands behind; inventing them here would have been worse than
  leaving the gap visible.
- **Checksums stay placeholders** for `better-launcher`, `better-awake`,
  `better-files`, and `better-storage`. ADR 0002 records the checksum of a
  published release asset, and no release carries these components. A hash
  taken from a local build would name an artifact nobody can download.
- **`docs/third-party-licenses.md` was stale on `main`** — it was generated
  before the crates tickets 18–35 added, so `build-deb.sh --check` refused to
  run and the CI package job would have failed on `main` for the same reason.
  Regenerated here; 876 resolved packages became 916.
- **No app-chooser binary is packaged.** `app-chooser-gui` is used by Better
  Files as a library, in process. Nothing spawns the binary, so shipping it
  would be a file with no caller.
- **dpkg does not stop a running user service at removal.** The manifests'
  remove and rollback plans own that; `better-awake.yaml`'s release notes
  describe manager behaviour, not package behaviour.

## What actually ran

- `packaging/build-deb.sh --output-dir dist/local --target local`: all eight
  packages built.
- `packaging/verify-deb.sh dist/local local`: all eight verified.
- The container apt install/remove smoke covers `better-launcher`, added to
  `packaging/test-daemon-e2e.sh` beside the existing `better-monitor` one. It
  needs no new base image and no network at test time because the launcher's
  runtime dependencies are the set the image already installs. It was **not
  run locally**: it needs Docker and the CI e2e client, and AGENTS.md forbids
  installing an unreleased build on the host. CI is where it first runs.
- arm64 and the Ubuntu 22.04 targets were not built here. This host is amd64
  and the script refuses cross-architecture packaging on purpose.
