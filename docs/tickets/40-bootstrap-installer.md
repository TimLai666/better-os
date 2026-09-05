# 40 — One-line bootstrap installer

**Epic:** Distribution: from a fresh Zorin/Ubuntu machine to a working Better
Manager in one command.
**User Story:** A user pastes one command, gets Better Manager (and its
daemon) installed with checksums verified, and installs everything else from
inside Better Manager.
**Blocked by:** none
**Status:** done

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

## What was built

- **`install.sh` at the repository root.** Bash, `set -euo pipefail`. It owns
  two packages and no more: `better-manager` and `better-manager-daemon`. Modes
  are `--dry-run`, `--uninstall`, `--from-dir <dir>`, and `--help`.
- **Distribution detection reads the base, not the badge.** `/etc/os-release` is
  parsed field by field rather than sourced, and the Ubuntu release comes from
  `UBUNTU_CODENAME` (`jammy` → 22.04, `noble` → 24.04) wherever there is one.
  Zorin OS 18.1 reports `VERSION_ID="18"`, which names nothing in the release
  matrix, so a `VERSION_ID`-first reading would have refused the project's own
  primary target. `VERSION_ID` is the fallback for plain Ubuntu only. Anything
  else, and any architecture outside amd64/arm64, is refused with the values it
  actually read printed back.
- **Release resolution needs nothing installed.** `curl` against
  `api.github.com/repos/TimLai666/better-os/releases/latest`, no `gh` and no
  token. `jq` is used when present and is not a dependency: without it the tag
  and the asset URL are matched by shape with grep and sed. The HTTP status is
  read rather than assumed, so 403/429 says the anonymous rate limit is spent
  and names the two ways out, 401 says the optional `GITHUB_TOKEN` is bad, and
  404 says there is no release yet. The download URL is looked up in the API
  response by asset name, so a release that does not carry this system's package
  is reported as missing rather than 404-ing mid-download.
- **Checksums are verified before anything is installed.** Both `.deb`s and both
  `.deb.sha256` sidecars land in a `mktemp -d` that is removed on exit; a
  mismatch stops the run and says nothing was installed.
- **One privileged command, printed first.** The command that needs root is
  built in one function and printed verbatim before sudo is asked for, so the
  statement cannot drift from what runs. It is a single
  `apt-get install -y --no-install-recommends` of the two verified files. As
  root already — which is how the container test runs it — sudo is not involved
  at all.
- **Idempotent.** Installed versions are read from dpkg before anything is
  downloaded. A machine already on the resolved version is told so and asked for
  nothing.
- **README and packaging spec.** The README quickstart carries the
  download-then-run one-liner and says why it is not `curl | sudo bash`.
  `docs/release-packaging.md` gains the installer contract: the ADR 0002 asset
  naming it depends on, the sidecar format, the release/architecture table it
  must be changed alongside, and where each half is verified.
- **CI.** A new `installer` job runs `shellcheck install.sh`, a fixture table
  that asserts Zorin 18 → 24.04, Zorin 17 → 22.04, Ubuntu 22.04 → 22.04, and
  refusal for Ubuntu 20.04 and Fedora, and a resolution run against the real
  public API with `jq` masked out of `PATH`. The package job adds a network-free
  `--from-dir --dry-run` over the packages it just built. The container
  end-to-end adds the half that changes a machine: a dry run that installs
  nothing, a deliberately tampered package that is refused before apt, the real
  install of both packages with an `ldd` check, an already-current second run,
  and `--uninstall`.

## Known limits

- The `sudo` branch is the one line of this script CI never executes. Both the
  container e2e and the runner jobs are root or dry runs, so what is tested is
  the command that gets built and printed, not the password prompt around it.
- `--dry-run` without `--from-dir` does not download, so it verifies no
  checksum. It prints the URLs it would fetch and says that it did not. The
  verification path is covered by `--from-dir` instead, where the bytes exist.
- The release table is a literal: `jammy` and `noble`, `amd64` and `arm64`,
  written in `install.sh` and in the packaging spec. Adding a release means
  editing both; nothing derives one from the other.
- `BETTER_OS_INSTALL_OS_RELEASE` exists so the detection table can be exercised
  against a machine's worth of fixtures. It chooses which published package is
  fetched and nothing else.
