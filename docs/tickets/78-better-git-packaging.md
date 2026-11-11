# Ticket 78 — Better Git is installable

**Blocked by:** 77 · **Status:** ready

## Why

Better Git builds and runs from the source tree only. The owner wants to open
it from the application menu in place of GitHub Desktop, which means a `.deb`,
a desktop entry, an icon, and a manifest so Better Manager can install it.

## What to build

Better Git becomes the ninth component, packaged the same way as the others.

- `packaging/git/better-git.desktop`: `Exec=better-git`, `Icon=better-git`,
  names and comment in English and zh_TW, `Categories=Development;RevisionControl;`,
  `StartupWMClass` matching the window's `app_id`.
- `packaging/icons/better-git.svg`, drawn in the style of the existing icons.
- `packaging/build-deb.sh` builds `git-gui` and makes `better-git_<version>_…deb`
  with the binary, the desktop entry, the icon, `LICENSE`, and the third-party
  notices, as every other package does. `git` is a `Depends`: the program
  cannot do anything without it.
- `packaging/verify-deb.sh` checks the package's file list and its desktop
  entry.
- `.github/workflows/ci.yml` builds, verifies, and uploads the package for all
  four targets.
- `components/manifests/better-git.yaml`, schema v2, at the workspace version
  with placeholder checksums, like the other eight before a release.
- Whatever lists the components by name (`manager-core` catalog, `install.sh`,
  `docs/release-packaging.md`, `docs/component-manifest.md`, `AGENTS.md`)
  gains Better Git, with the asset counts corrected (36 packages, 74 assets).
- A manifest test next to the others (`crates/git-gui/tests/manifest.rs`).

## Decisions

- Better Git is a `standalone` component, not an enhancement or replacement:
  it changes nothing else on the system.
- No privileged part. It never talks to `manager-daemon`.

## Acceptance

- [ ] `packaging/build-deb.sh` for the host target produces
      `better-git_<version>_<target>_<arch>.deb`, and `packaging/verify-deb.sh`
      passes on it.
- [ ] `dpkg-deb -c` lists the binary, the desktop entry, the icon, and the two
      license files; `dpkg-deb -f` shows `git` in `Depends`.
- [ ] `desktop-file-validate` accepts the desktop entry.
- [ ] The manifest loads through the same loader and tests the other manifests
      use, and Better Manager's catalog lists nine components.
- [ ] The package is extracted to a scratch directory and the binary inside it
      starts and prints `--version`. Nothing is installed on the host.
- [ ] fmt, clippy on stable and 1.99, the MSRV check, the workspace tests, and
      the license inventory check pass.
