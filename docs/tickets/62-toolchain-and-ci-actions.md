# Ticket 62 — The declared Rust version builds the workspace, and CI runs on current actions

**Epic:** Build and release · **User Story:** a contributor who installs the
Rust version the workspace declares can build it · **Branch:** `ticket-60-62` ·
**Blocked by:** none · **Status:** implemented, not merged

## What it delivers

The workspace declares `rust-version = "1.85"` and the locked dependencies need
at least 1.92 (`oo7 0.6.0`). CI uses stable Rust, so nothing notices. The
workflow's `actions/checkout@v4` and `actions/upload-artifact@v4` run on the
deprecated Node.js 20 runtime.

## Acceptance criteria

- [ ] `rust-version` equals the highest `rust_version` among locked packages,
      and every document that states the baseline says the same. (`Cargo.toml`
      now says 1.95, the version that actually builds, not the highest declared
      1.92; `AGENTS.md` and `delivery-status.md` still say 1.85. See What was
      built.)
- [x] CI gains a job or step that builds the workspace with exactly that
      toolchain, so the declaration cannot drift again.
- [x] Each action in `.github/workflows/` is on its current major version that
      runs on Node.js 24, checked against the action's own releases.

## What was built

The ticket's premise was one version short. The highest `rust_version` a locked
package declares is 1.92 (`oo7 0.6.0`), but the Zed git crates declare none,
and they are the ones that decide. With each toolchain installed and
`cargo check --workspace --locked` run against the unchanged tree:

- 1.92 fails in `gpui_util` on `slice::as_array`, stable from 1.93.
- 1.93 and 1.94 fail in `gpui` on `std::hint::cold_path`, stable from 1.95.
- 1.95 checks the whole workspace, all targets included.

Zed's own `rust-toolchain.toml` at the locked commit pins 1.95.0, which agrees.
So `rust-version` is now `1.95`, with a comment in `Cargo.toml` saying where the
number comes from. The first acceptance criterion's rule would have produced
1.92, which does not build.

Raising the declared version switched on clippy lints that only apply above
1.85: `collapsible_if` (let-chains, 1.88) and `manual_is_multiple_of` (1.87).
Under `-D warnings` they fail CI, so `cargo clippy --fix` applied them in
twenty files across `app-catalog-core`, `app-chooser-core`, `app-chooser-gui`,
`defaults-core`, `defaults-platform`, `files-operations`, `files-platform`,
`manager-core`, `manager-store`, `monitor-views`, `touchpad-core`,
`touchpad-gui`, `touchpad-platform`, and `touchpad-session`. Every change is a
mechanical rewrite with the same behaviour, and the workspace test suite passes
after it.

CI gained an `msrv` job. It reads `rust-version` from `[workspace.package]`,
installs exactly that toolchain with `dtolnay/rust-toolchain@master`, and runs
`cargo check --workspace --all-targets --locked`. Reading the number rather
than repeating it means the job and the declaration cannot disagree.

Every `uses:` in `.github/workflows/ci.yml` was checked against the action's own
repository on 2026-09-26:

- `actions/checkout` v4 → v7 (latest v7.0.1, `runs.using: node24`). v7's one
  behaviour change blocks checking out fork pull requests under
  `pull_request_target` and `workflow_run`, which this workflow does not use.
- `actions/upload-artifact` v4 → v7 (latest v7.0.1, `node24`). v7 adds an
  opt-in unzipped upload; the default is unchanged.
- `Swatinem/rust-cache@v2` stays; v2 (latest v2.9.2) already runs on `node24`.
- `dtolnay/rust-toolchain` is a composite action and runs no Node.js at all.

Deliberately not done: the `rust` job stays on stable, since that is what
contributors and releases use, and the MSRV job only checks rather than
building and testing, which would double the CI time for little extra proof.

Not verified: the new `msrv` job and the v7 actions have not run on GitHub yet.
`AGENTS.md` and `delivery-status.md` still describe the 1.85 baseline and the
Node.js 20 follow-up; this ticket was not allowed to edit them.
