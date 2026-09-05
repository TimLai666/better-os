# Better OS

Better OS is a modular performance-improvement layer for Zorin OS and Ubuntu.
It replaces, enhances, or diagnoses desktop and system components one workload
at a time. It is not a Linux distribution fork.

## Install

Zorin OS or Ubuntu, 22.04 or 24.04, on amd64 or arm64:

```bash
curl -fsSL -o /tmp/better-os-install.sh https://raw.githubusercontent.com/TimLai666/better-os/main/install.sh && bash /tmp/better-os-install.sh
```

The download and the run are two steps on purpose. Piping a URL into a root
shell executes whatever the network happened to return, unread and unverifiable;
this way the script is a file you can read before you run it, and it asks for
sudo itself rather than being handed a root shell.

What it does, in order:

- Works out which Ubuntu release this machine is built on. A derivative is
  identified by `UBUNTU_CODENAME`, not by its own version number — Zorin OS 18
  reports `VERSION_ID="18"` and `UBUNTU_CODENAME=noble`, which is Ubuntu 24.04.
  Anything outside 22.04 and 24.04, or outside amd64 and arm64, is refused with
  the values it read.
- Resolves the latest published release through the public GitHub API. No `gh`,
  no token, and no `jq` — `jq` is used when it happens to be installed.
- Downloads `better-manager` and `better-manager-daemon` with their `.sha256`
  sidecars into a temporary directory, and verifies both checksums **before**
  anything is installed. A mismatch stops the run with nothing installed.
- Prints the one command that needs root, then asks for sudo once and runs it:
  a single `apt-get install` of the two verified `.deb` files.
- Installs nothing else. Better Launcher, Better Monitor, Better Files, Better
  Touchpad, Better Awake, and Better Storage are installed from inside Better
  Manager, which is the point of installing it first.

Running it again on an unchanged release reports the machine as current and
asks for nothing. `--dry-run` prints every one of the steps above and changes
nothing. `--uninstall` removes the two packages it installed, and leaves
components installed through Better Manager alone.

## Current scaffold

- `better-core` validates versioned component manifests.
- `manager-core` plans install, update, enable, disable, verify, and restore,
  and owns the lifecycle state machine. Whether a stage actually happened is
  reported by a driver: a simulation scripts it, a real transaction observes it
  at the privileged boundary.
- `manager-store` persists versioned local state with atomic writes,
  stale-writer protection, corrupt-state backup, and restart resume.
- `manager-platform` owns the system capability, download, package, and
  privileged-executor interfaces. It downloads artifacts over HTTPS into a
  cache named by checksum, reads installed versions from dpkg, and talks to the
  privileged service. Applying a change requires an authorized connection to
  that service; every executor that can be built without one refuses.
- `manager-ipc` holds the wire contract the manager and the privileged service
  share, so both are generated from one definition.
- `manager-daemon` is the privileged service: a D-Bus system service authorized
  by polkit that revalidates every plan against the host, applies it through
  local APT, health-checks the result, and rolls back what it can on failure.
  It ships as its own `better-manager-daemon` package.
- `manager-cli` and `manager-gui` share the core API and never execute manifest
  lifecycle strings, APT, sudo, or shell commands themselves. Every privileged
  change goes through the service.
- `monitor-core` defines samples, incidents, inventory, and redacted exports.
- `better-ui`, `manager-gui`, and `monitor-gui` provide the GPUI application
  boundary. Better Manager is dark by default and also offers light and system
  appearances. It runs real transactions by default; a demo mode simulates them
  and says so on screen.

## Development

Run unreleased builds only in a disposable Chefer AppCipe, as required by
[`AGENTS.md`](AGENTS.md). The isolated verification command runs formatting,
workspace checks, tests, clippy, and CLI lifecycle smoke coverage without
installing a build or touching package state on the host.

Inside that disposable environment, use a scoped state path:

```bash
cargo run -p manager-cli -- --state-path /tmp/better-manager-state.json validate
cargo run -p manager-cli -- --state-path /tmp/better-manager-state.json run better-monitor install
cargo run -p manager-cli -- --state-path /tmp/better-manager-state.json status better-monitor
```

Read [`AGENTS.md`](AGENTS.md) before changing the project. The architecture
and current handoff state live in [`ENG.md`](ENG.md) and
[`delivery-status.md`](delivery-status.md). The manager screen behavior is
defined in [`docs/manager-ux-logic.md`](docs/manager-ux-logic.md). Accepted
decisions live in [`docs/decisions/`](docs/decisions/).
