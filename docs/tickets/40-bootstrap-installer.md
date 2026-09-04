# 40 — One-line bootstrap installer

**Epic:** Distribution: from a fresh Zorin/Ubuntu machine to a working Better
Manager in one command.
**User Story:** A user pastes one command, gets Better Manager (and its
daemon) installed with checksums verified, and installs everything else from
inside Better Manager.
**Blocked by:** none
**Status:** todo

## What it delivers

- `install.sh` at the repository root (served via
  `https://raw.githubusercontent.com/TimLai666/better-os/main/install.sh`):
  POSIX-sh compatible where practical, `set -euo pipefail` semantics,
  detects Ubuntu release (22.04/24.04, Zorin mapped to its base) and
  architecture (amd64/arm64), refuses unsupported combinations with a clear
  message, resolves the latest GitHub release via the public API without
  requiring `gh` or a token, downloads `better-manager` and
  `better-manager-daemon` `.deb`s plus their `.sha256` sidecars into a temp
  dir, verifies checksums before anything is installed, then installs via
  `apt-get install ./pkg.deb` (sudo requested exactly once, with an upfront
  statement of what will run as root; `--dry-run` flag prints everything and
  changes nothing).
- Never pipes untrusted content into a root shell beyond the two verified
  `.deb`s: the script itself instructs `curl -fsSL ... -o` then run, and the
  README one-liner uses the download-then-execute form rather than
  `curl | sudo bash`.
- Idempotent: re-running upgrades or reports already-current.
- `--uninstall` removes what it installed.
- README quickstart section updated with the one-liner and what it does;
  docs/release-packaging.md gains the installer contract (asset naming it
  depends on, per ADR 0002).
- CI: a job (or an extension of the container e2e) that runs `install.sh
  --dry-run` plus a container run of the real install path against locally
  built artifacts (network-free variant via a `--from-dir` escape hatch used
  only by tests).
- Shellcheck-clean (`shellcheck install.sh` in CI if available; local run at
  minimum).

## Out of scope

- A public APT repository (explicitly deferred by AGENTS.md decision).
- Package signing (deferred; checksums remain the mechanism).
- Installing other components (that is Better Manager's job — ticket 41).

## Verification

`bash -n` + shellcheck; `--dry-run` on the host; container run of the real
path with `--from-dir`; workspace gates untouched or green if code changed.
