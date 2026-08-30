# Better Files listing performance

What `cargo bench -p files-core` measures, what the numbers were, and which of
them are limits rather than achievements. The benchmark writes its own
synthetic fixtures each run, so nothing here depends on what happens to be on
the machine.

## How to reproduce

```
cargo bench -p files-core
cargo bench -p files-core -- --test   # single iteration on a smaller tree
```

The measurements are wall-clock timings and a counting global allocator, the
same style as `cargo bench -p app-catalog-core`. There is no benchmark harness
dependency: these numbers decide architecture questions, and a statistics
framework would not change any of the decisions.

## Fixtures

| Fixture | Contents |
| --- | --- |
| flat | 100,000 files in one directory |
| mixed media | 20,000 entries across ten extensions, one folder in twenty, varying file sizes |
| deep tree | fanout 4, depth 7; one level is listed, at the bottom |

Batch size is 256 entries, which is what a view would use.

## Results

Measured on the development host on 2026-08-30, median of 5 iterations, ext4 on
NVMe, warm page cache. Absolute numbers depend on the machine; the ratios are
the part that reflects the design.

### The flat 100,000-entry directory

| Measurement | Result |
| --- | --- |
| Time to first batch | 1.6 ms |
| Time to complete listing | 124.6 ms |
| First batch is sooner than completion by | 77x |
| Incremental sort, all 391 batches, one merge | 108.4 ms |
| Worst frame, coalesced (one merge per ~16 batches) | 28.3 ms |
| Whole directory assembled, coalesced | 356.5 ms |
| Worst frame, merging every batch | 27.5 ms |
| Whole directory assembled, merging every batch | 3954.5 ms |
| Model memory | 38.3 MB (402 bytes per entry) |
| Re-sort by name, reversed | 28.4 ms |
| Re-sort by size | 26.2 ms |
| Re-sort by modified | 50.5 ms |
| Toggle hidden entries | 0.45 ms |
| Cancellation latency | 0.021 ms |

The first number is the one the architecture exists for: a user waits about a
millisecond and a half to see the first 256 entries of a directory that takes
125 ms to read in full.

Cancellation latency is measured from the moment the token is set to the moment
the reader thread has actually exited, with the reader mid-directory. The
reader had delivered 256 of 100,000 entries and stopped there; the remaining
99,744 `stat` calls were never made.

### Mixed media, 20,000 entries

| Measurement | Result |
| --- | --- |
| Time to first batch, with MIME detection | 13.2 ms |
| Time to complete listing, with MIME detection | 372.6 ms |
| Time to complete listing, no detection | 25.1 ms |
| Incremental sort by type | 10.2 ms |

Type detection is by far the most expensive thing a listing can be asked to do:
it makes this directory about fifteen times slower to read completely. That is
why it is off by default and behind `ReaderConfig::with_mime`, and why it is
name-based — content sniffing would open every file.

### Deep tree

| Measurement | Result |
| --- | --- |
| Time to first batch, at depth 7 | 0.19 ms |
| Time to complete listing | 0.20 ms |

Listing one level costs what that level contains. Depth does not enter into it,
which is the expected result and is recorded so a regression would be visible.

## What these numbers changed

Two of them were bad enough to change the design rather than be published.

**The sort comparison allocated.** `natural_compare` built a `String` per digit
run and the tie-break built an `EntryId` per comparison. Across the tens of
millions of comparisons a streaming merge performs, that was most of the cost.
Both now compare in place. This alone took assembling the directory from
5,794 ms to 3,954 ms.

**Merging per batch is quadratic.** Every merge touches the whole ordered list,
so merging 391 times into a list growing to 100,000 entries did about twenty
million element moves and as many comparisons. `DirectoryModel::apply` now
*stages* a batch and `DirectoryModel::commit` merges; `Pane::pump` drains every
batch that arrived since the last frame and commits once. The row for "merging
every batch" is kept in the benchmark output precisely so the difference stays
visible: 3,954 ms against 356 ms.

## Known limits

- **A frame can still cost 28 ms while a very large directory loads.** The
  merge is proportional to the whole list, so the last commits of a 100,000-
  entry listing are the expensive ones. A consumer that pumps less often during
  a load pays less in total but more per frame; pumping more often is the
  reverse. Nothing here caches a precomputed sort key, which is the obvious next
  step if this becomes the limiting factor in the GUI. It is a follow-up, not
  something ticket 32 solved.
- **402 bytes per entry** is the model's own accounting and excludes the
  allocator's own overhead. A directory of a million entries would be roughly
  400 MB, which is not a size this design has been tested at.
- **Warm cache only.** Every number above was taken with the directory already
  in the page cache. Cold-cache listing over a real spinning disk or a USB stick
  is not measured here and should not be inferred from these figures.
- **Detection cost is the shared MIME database's**, as installed on this host.
  A host falling back to the small built-in extension table would be faster and
  would recognize less.
