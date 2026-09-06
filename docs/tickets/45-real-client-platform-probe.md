# 45 — The client asks the machine instead of assuming it

**Epic:** Field reports from the first real Zorin 18 install
**User Story:** The manager plans for the machine it is actually running on,
says which machine that is, and refuses honestly when the machine is not one
Better OS publishes for.
**Blocked by:** 44
**Status:** done

## The defect

`manager-gui` and `manager-cli` built their `Manager` from
`MockPlatform::default()`, which reports `ubuntu 24.04 amd64` on every machine
in the world. `Manager::probe` stores that profile, and
`Manager::artifact_for_profile` picks the artifact whose `release` and
`architecture` match it. So on any host that is not 24.04 amd64, every plan
named the wrong package.

It looked correct on the project's Zorin 18 host for one reason: ticket 43
taught the daemon to resolve that host to 24.04, so the client's assumption and
the daemon's answer happened to agree. On a 22.04 host the daemon refuses the
plan with `plan targets release 24.04 but this host is 22.04` — a message about
a mismatch, when the real problem is that the client never asked. On arm64 the
client would name an amd64 URL. The checksums cannot match across releases, so
nothing wrong could actually be installed, but the failure would arrive late
and describe the wrong thing.

`AGENTS.md` recorded it as a follow-up from ticket 44, which did not touch it.

## Where the answer now lives

There were two copies of "which Ubuntu release is this host built on" —
`install.sh` (ticket 40) and `manager-daemon/src/host.rs` (ticket 43) — and this
ticket needed a third. A third copy is how the client and the daemon drift
apart, so the rules moved into one place instead.

`crates/better-core/src/host.rs` owns them. `better-core` was the natural home
and the dependency direction allows it: `manager-daemon` and `manager-platform`
both already depend on it, and it depends on neither.

| Reader | How it gets the rules |
| --- | --- |
| `manager-daemon` | `better_core::host::resolve_ubuntu_release`, called by its own `parse_ubuntu_release`; every one of its five parser tests kept, unchanged |
| `manager-platform` (`HostPlatform`) | the same function |
| `install.sh` | shell, so it cannot share code. `detect_ubuntu_release` carries a comment pointing at the module, and every case in the shell mapping has a fixture test beside the Rust one |

The shared function is `install.sh`'s table, not the daemon's, because the
daemon's was the looser of the two. Three rules the daemon did not have:

- `VERSION_CODENAME` is accepted as a codename, but only for `ID=ubuntu`.
- The `VERSION_ID` fallback applies only to `ID=ubuntu`. A derivative's version
  is its own badge, not the base it was built from.
- The `VERSION_ID` fallback is accepted only when it names a release in the
  matrix.

Each of those tightens the daemon rather than loosening it: every case the
daemon used to resolve and now refuses was a case it would have refused one
step later anyway, with a worse message.

## What the client does with it

`crates/manager-platform/src/host.rs`:

- **`HostPlatform`** — release from `/etc/os-release`, architecture from
  `dpkg --print-architecture`, distribution from `ID`. Both host reads are
  read-only, so they stay unprivileged, which `AGENTS.md` allows explicitly.
  Free disk space stays `None`: nothing here measures it, and `None` means
  unavailable rather than zero.
- **`PlatformError::UnsupportedHost`** — a new error carrying
  `better_core::host::describe_os_release`'s one-line summary of the fields
  that were read. There is no fallback profile. Defaulting to 24.04 amd64 is
  the defect, not the recovery.
- **`ClientPlatform`** — `host()` or `demo()`. It exists so both clients pick
  the same way and so a test can assert which one a mode gets without reading
  the machine the test happens to run on.

**Distribution is now the real `ID`.** On Zorin that is `zorin`, not `ubuntu`.
This matters because `manager-core` gates planning on
`manifest.targets.distributions`: every first-party manifest declares
`[zorin, ubuntu]`, so nothing first-party changes, and `defaults-core` already
treats `zorin` as its own value. A third-party manifest that declared only
`ubuntu` would now be refused on a Zorin host — which is what the manifest
says, and was silently ignored before.

### The clients

| Site | Real mode | Demo mode |
| --- | --- | --- |
| `manager-gui/src/app.rs` | `ClientPlatform::host()` | `ClientPlatform::demo()` — a demo has to produce the same screens on any machine |
| `manager-cli/src/main.rs` | `ClientPlatform::host()` | `--execution mock` keeps the mock, and is therefore the one thing that still works on an unsupported host |

The CLI propagates an unsupported host as an error and stops. The GUI cannot:
a window has to draw something. `probe_manager` returns the manager plus an
optional `AppError::UnsupportedHost`, built from
`SystemProfile::unidentified()` — `unknown`/`unknown`/`unknown`, which no
manifest declares, so every plan fails with "this component ships nothing for
this host" rather than quietly targeting a release the machine is not running.
The banner says what Better OS publishes for, in both locales. An unidentified
host outranks a storage error in that banner, because it is why nothing on the
machine can be planned at all.

## The window says which machine it planned for

The sidebar footer already showed the profile. It showed `ubuntu 24.04` on
every machine, and on this host it was right by coincidence. It now shows what
was actually read. On the Zorin 18.1 host this ticket was written on, running
in real mode, it reads `zorin 24.04` over `amd64` — the derivative it is, the
Ubuntu base it plans against, and the architecture dpkg reported.

## Tests

Fixture-driven, hermetic, and never reading the machine the test runs on.

- **`better-core`, 13 new** — Zorin 17 and 18 (the verbatim 18.1 shape),
  plain Ubuntu with and without a codename, Ubuntu via `VERSION_CODENAME`, a
  derivative with no codename, an unknown codename, an Ubuntu release outside
  the matrix, Fedora, an empty file, field unquoting including the
  `ID`/`ID_LIKE` prefix trap, and the description string.
- **`manager-platform`, 6 new** — a Zorin 18 host resolving to noble and
  reporting itself as `zorin`, a jammy arm64 host (both halves differ from the
  mock's answer, so either one still being 24.04 or amd64 fails it), an
  unsupported host erroring with the host named and 24.04 nowhere in the
  message, an unreadable os-release, the real probe pointing at
  `/etc/os-release`, and the demo platform keeping the fixed profile.
- **`manager-gui`, 4 new** — the mode-to-platform mapping through
  `ClientPlatform`, a fixture host reaching the manager as 22.04/arm64/zorin,
  the unsupported-host state with nothing plannable and the release not falling
  back to 24.04, and the banner copy in both locales.
- **`manager-cli`, 2 new** — the same mapping, and an unsupported host refusing
  rather than planning.

`manager-daemon`'s five parser tests were kept exactly as they were and still
pass against the shared function, which is what proves the daemon's behavior
survived the move.

## Verification

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo check --workspace` | clean |
| `cargo test --workspace` | 167 targets, 2,555 tests, 0 failures |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| 8-second `ZED_HEADLESS=1` smoke, scratch XDG, `BETTER_MANAGER_OFFLINE=1` | window alive at 8s, no output |
| on-host run, real mode, scratch XDG, offline | footer read `zorin 24.04` / `amd64`; screenshot inspected |
| `manager-cli plan better-monitor install` on the host | planned `not installed -> 0.2.4` against the real profile |

## Not done

- No 22.04 or arm64 machine was available, so those paths are proved by fixture
  and not by a run. The fixture is the same text such a host would supply, but
  a fixture is not a machine.
- The architecture is passed through as dpkg reports it. An architecture
  outside the matrix produces "no artifact for this host" per component rather
  than one statement about the machine, which `install.sh` does give.
- `platform.unsupported_host` has no localized failure-evidence copy, so if it
  ever reached a failure card it would show the untranslated key — the same
  position `platform.capability_unavailable` is already in. It cannot reach one
  today: a manager only exists after the host was identified.
