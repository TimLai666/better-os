# Better Launcher performance

What `cargo bench -p launcher-gui --bench launcher_suite` measures, what the
numbers were, and — for the three that need a running process — what a headless
measurement can and cannot mean.

Until ticket 37 the launcher's manifest declared four benchmarks and nothing ran
any of them. This note is the other half of closing that: the harness produces
the numbers, `components/manifests/better-launcher.yaml` declares exactly the
five the harness produces, and `launcher-gui/tests/manifest.rs` fails if the two
lists drift apart.

## How to reproduce

```
cargo bench -p launcher-gui --bench launcher_suite
cargo bench -p launcher-gui --bench launcher_suite -- --quick     # smaller, seconds
cargo bench -p launcher-gui --bench launcher_suite -- --no-spawn  # model-only, no process
```

There is no benchmark-framework dependency, matching `launcher-core/benches` and
`files-gui/benches`. These are wall-clock timings of the real public API and two
`/proc` counters.

## Fixture

A synthetic XDG data directory written on every run: 5,000 generated `.desktop`
entries under `$XDG_DATA_HOME/applications`, with an empty `$XDG_DATA_DIRS` so
the host's own application list never enters any figure. Names repeat over a set
of twenty bases with an index appended, so query selectivity varies the way it
does on a real desktop, and every entry carries a `zh_TW` name, keywords, and a
category.

Five thousand is Issue #2's stated working target and is deliberately
pessimistic: a normal desktop has a few hundred entries. Read the open and
index-build figures as the ceiling, not the typical case.

## Hardware

Measured on the development host on 2026-08-31: AMD Ryzen AI 9 HX 370 (24
threads), 31 GiB RAM, ext4 on NVMe, Zorin OS 18.1, kernel 7.0.0-30-generic,
rustc 1.97.1, `bench` profile (optimized). Warm page cache throughout.

## Results

| Benchmark | Measurement | Result |
| --- | --- | --- |
| warm-search-update | keystroke to updated result model, p95 | **0.989 ms** |
| warm-search-update | the same, p50 | 0.066 ms |
| warm-search-update | the same, worst of 345 | 3.383 ms |
| warm-search-update | clearing the query back to the library, p95 | 0.336 ms |
| warm-search-update | index build behind the warm state | 108.8 ms |
| application-list-update | install to refreshed model, p95 | **151.8 ms** |
| application-list-update | removal to refreshed model, p95 | 151.8 ms |
| application-list-update | the same, minus the settle window | 1.8 ms |
| warm-overlay-open | spawn to first renderable model, p95 | **206.2 ms** |
| warm-overlay-open | the same, p50 | 181.5 ms |
| warm-overlay-open | of which `main()` to focused search row, p50 | 37.9 ms |
| warm-overlay-open | of which `main()` to library in the model, p50 | 179.0 ms |
| idle-overhead | CPU over a 20-second idle window | **0.0000 %** |
| idle-memory | resident set after that window | **52,992 kB** |

Issue #2 sets one absolute number, and it is the first row: warm search update
p95 below 50 ms over 5,000 records. It is met with about fifty times the margin.
Nothing else in the list has a documented absolute target; the manifest gives
each a regression budget instead, and the section on budgets below says why that
is not yet enforceable.

### Warm search update

The script types out seven queries a character at a time and deletes each one
back down: a single word, a multi-word phrase, an acronym, a CJK query, a long
specific query, one only a fuzzy match can answer, and one that matches nothing.
Every intermediate state is one timed `OverlayModel::set_query`, which is the
whole keystroke path the window uses — ranking, the result vector, and placing
the selection.

No warm-up is discarded, which is why the worst sample is 3.4 ms: the first
keystroke of the run is in there. The p50 of 0.066 ms is what typing normally
costs; the p95 of 0.989 ms is dominated by the queries that match thousands of
entries, since a wide result set is a large vector to build.

`launcher-core`'s own benchmark reported 1.005 ms p95 for the same script at the
index level. The GUI model layer adds the result clone and the selection lookup
and lands at 0.989 ms — the difference is run-to-run noise, which is the point:
the model layer adds no measurable cost on top of ranking.

Clearing the query back to the whole library costs 0.336 ms at p95 rather than
another index build, because the library rows are borrowed from the browse model
rather than rebuilt.

### Application-list update

An entry is written into a watched directory, the real `MetadataWatch` is waited
on, the catalog and index are rebuilt, and the result is applied to the model.
The clock stops when the model is showing the new list. Then the same for a
removal. Ten cycles, on a small directory so the reload is not the story.

**150 of the 152 milliseconds are a deliberate wait.** `launcher-platform`'s
`SETTLE` collapses a burst of filesystem events into one reload, because
installing a package writes many files and the library should be redrawn once.
The actual work — noticing the event, re-reading the directory, rebuilding the
index, and swapping it under the query the user already typed — is 1.8 ms. The
row that subtracts the settle window is in the output so the two never get
confused for each other.

The watcher backend was `EventDriven` (inotify) for every cycle. Nothing polls.

### Warm overlay open, and what "open" means without a compositor

The binary is launched with `ZED_HEADLESS=1`. There is no compositor, so there
is no surface and there is no frame. What is measured is **process start to
first renderable model**: the point at which the overlay entity exists, the
search row holds keyboard focus, and the application library is in the model —
everything a first frame would draw, drawn by nothing.

Inside the figure:

- fork, exec, and dynamic linking of the release binary
- a failed session-bus connection (see the exclusion below)
- GPUI application start and `gpui_component::init`
- constructing the overlay, focusing the search row, arming the watch
- reading 5,000 desktop entries and building the search index, on a background
  thread

Outside the figure, and not estimated anywhere:

- compositor handoff, surface allocation, and the first buffer swap
- GPU pipeline and font atlas warm-up that a real surface would force
- present-to-photon, and therefore anything a user would call "how long until I
  see it"

`main()` to a focused search row is 37.9 ms. That is the number that matters for
the interaction Issue #2 describes, because the search row takes focus before
the list exists and someone can start typing into a library that is still
loading. Reaching the full library takes 179.0 ms at 5,000 entries, essentially
all of it the 108.8 ms index build plus reading the directory.

Two spawn-time choices are worth stating. The first spawn of each run is
discarded, because the very first execution of a freshly built binary pays to
read it off disk and "warm" is the claim being made. And the spawned process is
pointed at an unreachable `DBUS_SESSION_BUS_ADDRESS`, so it deterministically
takes the "no session bus, opening without single-instance" path instead of
forwarding its request to whatever else on the machine owns the name. A failed
bus connection is therefore inside the figure and a successful one is not.

### Idle CPU and memory

The overlay is opened, given two seconds to settle so that opening is not
charged to idle, and then left alone for twenty seconds.

Over that window the process used **no measurable CPU at all**: zero nanoseconds
in `/proc/[pid]/schedstat` and zero clock ticks in `/proc/[pid]/stat`. Both are
reported because either alone can mislead — the tick counter could only have
said "0 % or 0.05 %" over this window, and a schedstat that a kernel never
populated would read as a permanent zero. The harness also prints how much the
same schedstat counter had already accumulated during startup (76.5 ms), which
is what makes the zero evidence about the process rather than about the counter.

That result is what the design predicts. The watch task blocks on the kernel's
event channel with a one-hour re-arm rather than polling, and nothing else in
the overlay runs without input.

Resident set was 52,992 kB with 5,000 applications indexed, and peak equalled
final, so nothing transient was larger than the steady state.

## Known limits

- **Headless idle is not session idle.** With no compositor nothing asks the
  window to repaint. A real session redraws on damage, and cursor movement,
  theme changes, and monitor configuration events all reach a live window. The
  0 % figure is the launcher's own idle cost with a warm index; it is not a
  claim about a launcher sitting on a running desktop.
- **The overlay is transient in this build.** The process exists while the
  overlay is on screen and ends with it, so "idle" is a state of seconds, not
  hours. If the launcher ever becomes resident, this measurement has to be
  retaken over a much longer window, and memory would need a leak check rather
  than a single reading.
- **No frame is measured, anywhere.** Every figure above stops at a model. No
  Better OS benchmark has measured time-to-photon for any component, and this
  one does not start.
- **Warm cache only.** The fixture is written immediately before it is read, so
  the directory and every entry are in the page cache. A cold read of
  `/usr/share/applications` on a slow disk is not measured here.
- **5,000 entries is the ceiling, not the desktop.** Open time and index build
  scale with entry count. A machine with 300 applications will not see 179 ms.
- **The regression budgets are not enforced.** The manifest declares a maximum
  regression per benchmark, and comparing against it needs a stored baseline and
  a CI job that runs the harness. Neither exists. This run is the first
  candidate baseline; nothing consumes it yet. Better Files has the same gap and
  it is recorded once, for both.
- **One machine.** Every number is from the host described above. Nothing here
  has been run on arm64, on a slower disk, or under memory pressure.

## What is not compared

Nothing here is measured against the GNOME application overview or any other
launcher. Better Launcher enhances the overview rather than replacing it, and a
comparison would need a defined workload, a way to measure another program's
time-to-first-content, and hardware fixed for both. None of that exists, so no
comparison is stated.
