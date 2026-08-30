# Better Files benchmarks

Issue #6 asks for a repeatable benchmark suite over a named list of scenarios,
and states that any public performance claim must include its workflow, dataset,
hardware, and metric. This file is where the numbers and that methodology live.

## Running it

```
cargo bench -p files-gui --bench files_suite
cargo bench -p files-gui --bench files_suite -- --quick   # smaller fixtures, one iteration
```

One command, one summary table. Two narrower harnesses exist beside it and are
still the right place for their own detail: `cargo bench -p files-core`
(listing and model), `cargo bench -p files-gui --bench view_model` (the row and
selection layer), `cargo bench -p files-operations` (the job engine), and
`cargo bench -p files-preview` (per-provider preview cost).

## Methodology

Every number below is **this machine, this filesystem, warm page cache, release
build**. That is a real limitation and it is stated rather than buried:

| | |
| --- | --- |
| Host | Zorin OS 18.1, Linux 7.0.0-30-generic, x86_64 |
| Filesystem | the developer's own, through `std::env::temp_dir()` |
| Build | `cargo bench`, release profile |
| Cache state | warm. Nothing drops caches between iterations. |
| Iterations | 5 for the directory and preview scenarios, 1 for the copies |
| Statistic | median, except where the row says p50 or p95 |

**The copy figures are page-cache figures.** 6,170 MB/s is memory bandwidth, not
a disk. No spinning disk, USB device, or network share has been measured, and no
claim about copy speed should be made from this table without saying so.

**The device figures are model figures.** Unplugging a real disk mid-write is
not a repeatable benchmark, so the connect/open/disconnect cycle is driven
through `storage-core`'s state machine. What it measures is the cost of the
model, not the latency of the bus or of UDisks2.

## Results

Recorded 2026-08-30, 100,000 entries, 100,000 small files, 512 MB large copy.

| Scenario | Metric | Value |
| --- | --- | --- |
| 100,000-entry directory | time to first visible entries | **3.662 ms** |
| 100,000-entry directory | time to complete model | **155.032 ms** |
| 100,000-entry directory | one row lookup while fully loaded | **< 0.001 ms** |
| Multi-tab navigation | four tabs over the same directory to complete | **411.660 ms** |
| Startup, unavailable location | time to reported failure | **0.676 ms** |
| Search in 100,000 entries | keystroke p50 | **0.001 ms** |
| Search in 100,000 entries | keystroke p95 | **0.002 ms** |
| Search in 100,000 entries | first slice of results (4,096 candidates) | **0.959 ms** |
| Search in 100,000 entries | all results (90,000 matches) | **54.879 ms** |
| Preview generation | 1920×1080 PNG, p95 | **16.693 ms** |
| Preview generation | 128 KiB read from a 5 MB source file, p95 | **0.072 ms** |
| Preview generation | folder summary over 100,000 entries, p95 | **20.075 ms** |
| Large sequential copy | 512 MB, page cache | **6,170 MB/s** (82.985 ms) |
| Many small files | 100,000 files | **24,335 files/s** (4.109 s) |
| Same-filesystem move | 512 MB | **2.077 ms** (a rename; no bytes copied) |
| Device connect/open/disconnect | per cycle, 2,000 cycles | **0.001 ms** |
| Device state event | per mount event | **< 0.001 ms** |
| Sidebar inventory rebuild | per rebuild | **< 0.001 ms** |

## What the numbers say

**The architecture claim holds at the top of the list.** First content in 3.7 ms
on a hundred thousand entries, and the whole model in 155 ms, with the listing
never on the render thread. The number that matters for the feel of it is the
third row: a row lookup at full size is below the timer's resolution, because
the view formats a screenful per frame rather than a directory.

**Typing does not block, by construction.** A keystroke costs 0.002 ms at p95
because it allocates a run and nothing else. The scan is spread at 4,096
candidates per frame, so the first results are visible after 0.959 ms and a
search that matches 90% of a hundred thousand entries finishes in 55 ms of work
spread across about 22 frames.

**Preview is the slowest thing here, and that is the right shape.** A 2-megapixel
PNG decode is 17 ms — far too long for a render thread and entirely fine on a
worker, which is exactly why it is on one. The folder summary over 100,000
entries at 20 ms is the one to watch: it is a `readdir` plus a `stat` per entry,
and it is bounded by `max_folder_entries` rather than by being fast.

**The small-file copy is the honest weak spot.** 24,335 files per second means
100,000 files takes four seconds, and that is with the page cache warm and no
device-level flush. This is the scenario where a real device will look nothing
like this table.

## Scenarios Issue #6 names that this harness does not yet cover

Stated rather than quietly dropped:

- **Mixed media with thumbnails.** `files-preview` generates thumbnails and the
  per-image cost is measured, but the view does not request them for a grid yet,
  so there is no thumbnail-under-scroll scenario to measure.
- **Deep directory tree.** The listing is per-directory, so a deep tree costs
  what its directories cost; there is no recursive traversal in the read path to
  measure. A recursive search would need one, and that is the same follow-up.
- **Cross-filesystem move.** Needs a second filesystem the harness can rely on
  existing. `files-operations` covers the copy-then-delete path in its own
  tests; the timing is not measured here.
- **Scrolling frame time and dropped frames.** Needs a window and a compositor.
  The proxy measured here is the per-row cost the frame pays, which is what the
  view-model benchmark reports in more detail.
- **Cold startup, memory use, CPU use, idle overhead.** Needs process-level
  instrumentation rather than in-process timing.

## The comparison Issue #6 asks for

**Not measured, and deliberately not estimated.** Issue #6 asks for a comparison
against Nautilus, COSMIC Files, and Windows File Explorer on equivalent
workloads. Producing one honestly needs three things this environment does not
have:

1. A defined dataset and a defined machine that both sides are run on, rather
   than a developer laptop with a warm cache.
2. A way to measure another program's time-to-first-content that is not a person
   with a stopwatch. Nautilus does not expose one.
3. For Windows File Explorer, hardware with a filesystem-equivalent workload,
   which Issue #6 itself hedges with "where practical".

Fabricating a comparison would be worse than not having one, so the follow-up is
recorded here and in `AGENTS.md`: **the comparison hardware and datasets need a
decision before any public performance claim names another file manager.** Issue
#6 already lists this among its deferred decisions.

## Regression budgets

`components/manifests/better-files.yaml` declares a budget for each of the
scenarios above under `benchmarks`. Nothing enforces them yet — there is no CI
job that runs this harness and compares — which is the second follow-up.
