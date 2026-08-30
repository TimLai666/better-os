# Application identity model

Better Files, Better Launcher, Better App Chooser, and every future Better OS
surface that lists installed applications read one catalog. This note records
what an application *is* in that catalog, and exactly how far each consumer may
reach into it.

## An application is a desktop ID, never a path

The canonical identity of an application is its desktop file ID: the entry's
file name with any subdirectory folded into `-`, keeping the `.desktop` suffix.
`applications/kde4/konsole.desktop` is `kde4-konsole.desktop`.

A path is not an identity. Nothing that stores a selection, a MIME association,
a favorite, or a launcher history entry may store a path in place of the ID.
The reasons are structural, not stylistic:

- A Flatpak, Snap, or D-Bus-activated application has no single executable to
  point at.
- The same ID can be provided by several files at once, and which file wins
  depends on directory precedence that can change when a user installs
  something.
- A path invites a consumer to run it directly, skipping the launch semantics
  the entry declared.

## Executable resolution is a reported status

`ExecutableStatus` has three shapes and no fourth:

| Status | Meaning |
| --- | --- |
| `Resolved(path)` | A regular, executable file was found at this exact path on this host. |
| `Unresolved { program }` | The entry names one program and it was not found. |
| `NotApplicable { reason }` | No single canonical executable exists: Flatpak, Snap, AppImage, wrapper, or D-Bus activation. |

`ExecutableStatus::path()` returns a path only for `Resolved`. There is no code
path that constructs a path from a program name without checking that the file
exists and is executable, and no code path that turns `NotApplicable` into a
path at all. A future "Choose Executable" surface reads this status and refuses
rather than inventing an answer.

## Visibility is evaluated, not baked in

Five rules decide whether an application is shown, and they are not equivalent:

| Rule | Effect |
| --- | --- |
| `Hidden=true` | The ID is **deleted**. A user entry with `Hidden=true` removes the system application of the same ID from the catalog entirely. |
| `NoDisplay=true` | The record exists and is launchable, but is not a menu item. |
| `OnlyShowIn` | Excluded unless the current desktop is named. |
| `NotShowIn` | Excluded when the current desktop is named. |
| `TryExec` missing | Excluded: the entry itself says its program is not installed. |

`Hidden` is resolved when the catalog is assembled, because it changes which
records exist. The other four are evaluated per query against the current
`XDG_CURRENT_DESKTOP`, because the same catalog can be asked by more than one
consumer and a consumer may legitimately want the excluded ones — a chooser
showing "all applications" is not a menu.

`Terminal=true` is carried, never used to exclude. A terminal application is a
real application; the launcher wraps it in a terminal emulator's argument
vector.

## Precedence and shadowing

Directories are ranked: `$XDG_DATA_HOME/applications` first, then each
`$XDG_DATA_DIRS` entry in the order the variable lists them. The lowest-ranked
directory holding an ID owns it. Losing entries are kept as `shadowed()` and
rejected files as `rejected()`, each with a stable machine key, so "why is this
application missing" is answerable without re-reading the disk by hand.

## Launching goes through the definition

A launch produces one of exactly two typed plans:

- `LaunchPlan::Process` — argument vectors, one per process to start. An entry
  declaring `%f` handed three files produces three invocations, because that is
  what the specification requires.
- `LaunchPlan::Activation` — a D-Bus call to `org.freedesktop.Application`,
  addressed by the well-known name derived from the desktop ID.

There is deliberately no variant carrying a command string. The `Exec` value is
tokenized once, at parse time, into argument pieces; selected files and URIs are
substituted into those pieces afterwards. A file named `a b; rm -rf ~.txt`
therefore arrives as one argument, and the launch smoke test proves it by
reading back the `argv` a real launched process received.

When D-Bus activation is requested but no activator is configured, the launcher
falls back to the entry's own `Exec` line, which the specification requires a
`DBusActivatable` entry to keep for exactly this case. The fallback is reported
in the outcome rather than hidden.

## The boundary each consumer may cross

| Consumer | May do | May not do |
| --- | --- | --- |
| Better App Chooser | read records, filter by MIME type, return a `DesktopId` plus an optional action ID | parse a `.desktop` file, return a path as the selection |
| Better Launcher index | read records, build its own index and ranking over them | scan XDG directories itself, cache parsed entries in its own format |
| Better Files Applications location | read records, render them, launch through the shared launcher | expose the backing `.desktop` file as if it were the application |
| Any GUI crate | call discovery on a background thread | run discovery on the render thread, or depend on GPUI from either catalog crate |

Neither `app-catalog-core` nor `app-catalog-platform` depends on GPUI, and
neither knows what a render thread is. Discovery is plain blocking I/O that a
consumer schedules where it likes.

## Deliberately not decided here

Issue #4 requires an ADR or a focused follow-up rather than a silent choice for
each of these, and none of them is encoded in the record model:

- the category and grouping design,
- whether a public `applications://` URI should exist,
- how favorites and usage history are shared with Better Launcher.

## Benchmarks

Measured with `cargo bench -p app-catalog-core`, median of 10 iterations over a
5,000-record synthetic catalog written fresh to a temporary directory (4,750
system entries including 500 in a nested vendor subdirectory, 250 user entries,
plus 250 user entries that override system entries of the same ID, giving 5,250
distinct records). Each entry carries three localized names, two desktop
actions, three MIME types, and a `TryExec`.

Hardware: AMD Ryzen AI 9 HX 370 (24 threads), 30 GiB RAM, ext4 on NVMe,
Zorin OS 18.1, Linux 7.0.0-30-generic, rustc 1.97.1, release profile.

| Measurement | Result |
| --- | --- |
| Cold discovery (first walk of a freshly written tree, one sample) | 44.6 ms |
| Warm load (full re-discovery, page cache warm) | 40.4 ms |
| Warm load with the real host executable probe | 153.5 ms |
| Refresh after an entry is added | 40.5 ms |
| Refresh after an entry is removed | 40.1 ms |
| Visibility filter over a live catalog | 0.018 ms |
| MIME-type filter over a live catalog | 0.055 ms |
| Memory footprint of one live catalog | 20.1 MB (4,019 bytes per record) |

Cold discovery is a single sample and moved between 44 ms and 59 ms across
runs, because the tree is written immediately before it is read and how much of
it is still in the page cache is not under the benchmark's control. The gap
between it and the warm number is the honest bound on that effect, not a
measurement of disk latency on a cold machine.

Two things the numbers say:

- The executable probe, not parsing, dominates a full load: resolving 5,000
  program names against the real `PATH` costs roughly 113 ms of `stat` calls.
  A consumer that does not need executable paths should discover with a probe
  that resolves nothing.
- Filtering a live catalog is three orders of magnitude cheaper than loading
  one, so consumers should share one catalog and filter it rather than each
  holding a copy. Issue #4 asks for exactly that.

Change watching uses `notify`'s recommended backend, which is inotify on Linux.
Nothing runs while the directories are idle; a test asserts the backend is
event-driven rather than a poll watcher, because a poll watcher would silently
turn an idle desktop into a busy one.
