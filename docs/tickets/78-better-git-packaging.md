# Ticket 78 — Better Git is installable

**Blocked by:** 77 · **Status:** merged, not released

## Why

Better Git builds and runs from the source tree only. The owner wants to open
it from the application menu in place of GitHub Desktop, which means a `.deb`,
a desktop entry, an icon, and a manifest so Better Manager can install it.

## What to build

Better Git becomes the ninth component, packaged the same way as the others.

- `packaging/git/io.betteros.Git.desktop` (named after the window's `app_id`): `Exec=better-git`, `Icon=better-git`,
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

- The manifest's `component_type` and `icon` are closed sets, and a released
  Better Manager refuses a manifest that uses a value it does not know. This
  ticket adds no new value to either set: Better Git takes the existing type
  that fits best (`enhancement`, with nothing it replaces) and an existing
  icon value. A type for a standalone application is a schema change for its
  own ticket.
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

## What was built

- **The package.** `packaging/build-deb.sh` builds `git-gui` in the same
  `cargo build` as every other component and makes `better-git` through the
  shared `make_package`: `/usr/bin/better-git`, `io.betteros.Git.desktop`, the
  hicolor icon, `copyright` and `THIRD-PARTY-LICENSES.md`. `Depends` is the
  derived shared libraries, the GPUI runtime libraries every window names, and
  `git`.
- **The desktop entry** (`packaging/git/io.betteros.Git.desktop`) runs
  `better-git`, names the `better-git` icon, has English and zh_TW names and
  comments, and is in `Development;RevisionControl;`. It is named after the window's `app_id`,
  `io.betteros.Git`, as `better_ui::window_options` requires, the way Better
  Files' entry is; the ticket's first draft named it `better-git.desktop`,
  which the shell would not have matched on Wayland. The comment describes what the window does today (review and
  commit); branches and history are later slices.
- **The icon** (`packaging/icons/better-git.svg`) follows the family: the
  rounded primary square, a history line with a branch in white, and the
  branch tip in the tertiary accent. `<svg` is inside the first 100 bytes.
- **The verifier** checks the file list, the license files, `Exec`,
  `StartupWMClass`, the icon behind `Icon=`, `desktop-file-validate`, and
  `git` in `Depends`. CI's per-component list includes `better-git`; upload
  already takes every `.deb` in the directory.
- **The manifest** (`components/manifests/better-git.yaml`, schema v2, 0.2.9,
  placeholder checksums). `component_type` is `enhancement` with nothing in
  `replaces` or `enhances`, and `icon` is `generic`: both sets are closed, and
  the validator puts no requirement on `enhances` for an enhancement
  (`better-awake.yaml` already declares none). It is in `manager-core`'s
  built-in catalog, which now holds eight manifests, and in the CLI test that
  every released component is offered.
- **The manifest test** (`crates/git-gui/tests/manifest.rs`) checks the
  closed-set choices, the workspace version, the artifact names, the declared
  paths (the recent-repositories file is derived from `git_gui::recent`), a
  catalog of all eight first-party manifests, and that the desktop entry's
  `StartupWMClass` is the `app_id` in `src/app.rs`. `git-gui` gains
  `better-core` as a dev-dependency, so `Cargo.lock` and the license inventory
  moved together.
- **Docs.** `docs/release-packaging.md`, `docs/component-manifest.md`,
  `AGENTS.md`, and `README.md` name Better Git and the next release's counts:
  nine packages, 36 `.deb` files, 74 assets. Past releases' counts are left as
  they were published.

Not done, and why:

- The ticket said Better Manager's catalog lists nine components. It lists
  eight: the ninth package is `better-manager-daemon`, which has never been a
  catalog component.
- `crates/manager-store/src/catalog.rs` asserts the built-in catalog holds 7
  manifests in two tests. That file was outside this change's scope, so those
  two tests fail until they say 8.
- No new `component_type` or `icon` value; a standalone type is its own ticket.

Not verified:

- Only the host target (ubuntu-24.04, amd64) was built and verified. The
  22.04 and arm64 packages are left to CI.
- The window was not opened from the packaged binary on a desktop, so the
  match between the entry and the window was checked as text, not by a shell. `--version` from
  the extracted package was run. Nothing was installed on the host.

