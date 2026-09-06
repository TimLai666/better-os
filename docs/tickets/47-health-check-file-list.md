# 47 — The health check asks dpkg what the package installed

**Epic:** Field reports from the first real Zorin 18 install
**User Story:** Installing a component that ships several binaries succeeds and
stays installed, and the window footer says what the machine is without
inventing a version of it.
**Blocked by:** 45
**Status:** done

## The defect

`manager-daemon/src/health.rs` derived one path from the package name:

```rust
fn binary_path(component: &str) -> PathBuf {
    PathBuf::from("/usr/bin").join(component)
}
```

Three of the eight shipped packages are not shaped like that.

| Package | What it installs | `/usr/bin/<package>`? |
| --- | --- | --- |
| `better-awake` | `better-awake-service`, `awake-tray`, `awake-gui` | no |
| `better-storage` | `better-storage-service`, `better-storage-doctor` | no |
| `better-manager-daemon` | `/usr/libexec/better-manager-daemon` — started by D-Bus, not by a person | no |

So on the field
machine running 0.2.5 every install of Better Awake passed apt, failed its
health check with `/usr/bin/better-awake is missing`, and was rolled back — the
daemon honestly removing what it had just installed. The GUI showed
「元件未通過健康檢查」, which was true and told nobody what was actually wrong.

The rule was not wrong about trust. It was wrong about packages: a package name
does not name a file, and only dpkg knows what a package put on disk.

`better-manager-daemon` is in that list and is worth stating plainly: the
manager could never have updated its own privileged service through a
transaction either, for the same reason and with the same rollback.

## The new rule

`health::check` for Install, Update, and Restore now passes when **both** hold:

1. `dpkg-query -W` reports the package installed, and
2. every path `dpkg-query -L` lists for it is on disk.

with three qualifications, each one a deliberate decision rather than an
oversight:

| Listed path | Treated as | Why |
| --- | --- | --- |
| A directory | skipped | Directories are shared between packages and say nothing about this one |
| A conffile (from dpkg's own `${Conffiles}`) | skipped | dpkg lets a machine's owner delete a configuration file and does not consider the package broken; failing there would report the admin's choice as a broken install |
| A path this machine's dpkg was configured never to unpack | skipped | see below — this one nearly shipped as a worse version of the bug being fixed |
| A symlink, including a dangling one | present | The link is the file the package installed. `symlink_metadata` is what the probe calls, so nothing follows a link out of the package |

A package whose file list contains no regular files at all is healthy on the
same rule — everything it listed is there — which is the honest answer for a
metapackage rather than a special case.

Remove is unchanged: healthy when dpkg no longer reports the package.

## The path-exclude trap

**dpkg lists files it was configured never to install.** `--path-exclude` drops
a file at unpack and leaves it in the package's file list, so `dpkg-query -L`
names a file that was never written. Minimized Ubuntu images — including the
`ubuntu:24.04` image the container end-to-end check runs in — ship
`/etc/dpkg/dpkg.cfg.d/excludes` with `path-exclude=/usr/share/doc/*`, and every
Better OS package installs `THIRD-PARTY-LICENSES.md` there.

The first version of this fix would therefore have failed **every** package on
every minimized machine: a stricter, more widespread version of the defect it
was written to remove. It was caught by testing dpkg rather than trusting a
recollection of it — a fixture package installed into a throwaway `--root` with
the Docker image's own filters, whose `dpkg-query -L` listed the excluded file
and whose disk did not have it. `dpkg --verify` reports it as `missing` too, so
there is no dpkg query that answers this; the configuration is the only thing
that knows.

`crates/manager-daemon/src/dpkg_config.rs` reads it: `/etc/dpkg/dpkg.cfg.d/*` in
name order (only names dpkg itself accepts — no dots) and then
`/etc/dpkg/dpkg.cfg`, with the last matching rule deciding, and `apt-config dump
DPkg::Options` on top, because this driver installs through `apt-get` and apt's
dpkg options reach dpkg whether or not the driver named them. The glob rules are
dpkg's own, where `*` crosses `/`. A machine with no filters — an ordinary
desktop, including the field machine — gets an empty filter and loses nothing.

`Undetermined` semantics are preserved and now cover one more failure. A
version query that fails is `Undetermined`, as before, and a file-list query
that fails is `Undetermined` too, so a broken dpkg database can never be read
as a passing check.

The trust model did not move. The paths come from dpkg's record of the package
the daemon just verified by checksum and installed itself. Nothing a manifest
wrote reaches the check, and nothing is executed — the probe stats paths and
reads nothing else.

## Where the code went

| File | Change |
| --- | --- |
| `crates/manager-daemon/src/apt.rs` | `AptDriver::installed_files` returns `InstalledFiles { paths, conffiles }`, backed by `dpkg-query -L` and `dpkg-query -W -f='${Conffiles}'`; `parse_file_list` keeps only lines that start at the root, so dpkg's diversion prose is not mistaken for a path |
| `crates/manager-daemon/src/health.rs` | `binary_path` deleted; `HealthProbe` answers `PathState::{Missing, Directory, Present}` for an arbitrary path instead of "is this an executable file" |
| `crates/manager-daemon/src/executor.rs` | unchanged except for the probe constructor — the executor never knew which paths were checked |

`dpkg-query -L` rather than `/var/lib/dpkg/info/<pkg>.list` read directly: the
list file is dpkg's private storage, its name is not stable for a package with
an architecture qualifier (`better-monitor:amd64.list`), and it carries no
conffile information. The query tool answers both questions from the same
database and is what the rest of the driver already uses.

One implementation detail worth keeping: a query's output is **not** truncated
to the log tail. `AptRun` keeps only the last `MAX_LOG_OUTPUT_BYTES` of a
command's output, which is right for a transaction log and wrong for a file
list — a truncated list would silently check fewer files than the package
installed. `AptGetDriver::query` runs the same command and returns all of it.

## Tests

`dpkg_config` carries 7 of its own, including the Docker image's `excludes`
file verbatim (documentation dropped, `copyright` kept by the later
`path-include`, and nothing a package actually needs excluded), a config
directory read in order, and the glob rules.

Daemon unit tests, 6 new and 1 rewritten:

- an awake-shaped package — three binaries, none named after the package —
  passes;
- a package with a listed file missing fails, and the failure names that file;
- a deleted conffile is not a broken package;
- a path-excluded file is not a missing file;
- an unreadable file list is `Undetermined`;
- `SystemHealthProbe` against a real temporary directory: a directory is a
  directory, a file is present, a **dangling symlink** is present, an absent
  path is missing;
- removal is unchanged, and its test is untouched.

Plus `executor`: a whole transaction installing an awake-shaped package
succeeds and is not rolled back — the field failure, at the level it was
reported. And in `apt`: the file-list and conffile parsers, including dpkg's
diversion line and an `obsolete` conffile.

`FakeAptDriver` gained `with_files` and `with_unreadable_files`. A package with
no declared file list reports one binary named after itself, which is the
ordinary single-binary shape and keeps every existing test meaning what it
meant.

## Container end to end

The e2e check's authorized path installed `better-monitor`, which passed the
old rule, so this whole class of defect could pass through it unseen. Two
changes close that:

- **`better-awake` is now installed and removed through the privileged
  service** in `packaging/test-daemon-e2e.sh`. It must report `succeeded` and
  `healthy`, `/usr/bin/better-awake` must **not** exist — asserted, so that if
  the package ever grows an eponymous binary the test says it no longer proves
  anything — and every path `dpkg-query -L better-awake` lists is checked
  against the real filesystem — `/usr/share/doc` excepted, because the image
  drops it, which is the same reason the daemon reads dpkg's filters and makes
  this run a real test of that path rather than an accident. It costs no new
  infrastructure: `dpkg-shlibdeps`
  gives `better-awake` the same graphics libraries the image already installs
  for `better-monitor` and `better-launcher`, so there is nothing to fetch at
  test time.
- **The rollback fixture is now unhealthy for the right reason.** Its 0.1.0
  package used to ship no files at all, which under the new rule is healthy. It
  now ships two files and a postinst that deletes one of them after dpkg has
  recorded it, which is exactly the state the check exists to catch: dpkg says
  installed and lists a file that is not there. Both fixture versions are also
  named unlike their own binaries, so the 0.0.9 install passing its check
  exercises the new rule against real dpkg state.

Neither ran here: this worktree has no Docker daemon, so the container e2e is
CI-only until someone runs it. What ran locally is the Rust gate below.

## The footer

Separately, and small: the window footer read `zorin 24.04`, which is the
distribution ID beside the Ubuntu release the packages are built for. It reads
as "Zorin version 24.04", which does not exist.

`better_core::host::describe_distribution` now returns the host's own name and
badge — `Zorin OS 18`, `Ubuntu 24.04` — from `NAME` and `VERSION_ID`, and
`SystemProfile` carries it as `distribution_label`, a display value that is
never matched against a manifest. `manager_gui::model::host_line` composes the
line:

| Host | Footer |
| --- | --- |
| Zorin OS 18 on noble | `Zorin OS 18 · Ubuntu 24.04 base` (`… Ubuntu 24.04 基礎` in zh-TW) |
| Plain Ubuntu 24.04 | `Ubuntu 24.04` — one fact, shown once |
| Unidentified host | `unknown`, naming no release at all |

Both lines in the footer truncate rather than widening the sidebar rail.

## What is not proved

The footer string is asserted by test from the real host probe's own output,
for both locales and both host shapes. It has **not** been looked at on a
running desktop in this worktree, and the container e2e has not been run here.
