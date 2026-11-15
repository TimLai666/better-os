# Better Storage performance

## What is recorded

Synthetic, no hardware (ticket 31, release build, one machine):

| Benchmark | Result |
|---|---|
| state-machine throughput | 7.3M events/s, 136 ns per event |
| event-to-state-update latency through the service | p50 17 µs, p99 23 µs |
| idle service | no timer, no published update, no state change |

These are the three the manifest declares. They say nothing about a device.

## Real devices: not measured yet

No real-device number exists. The harness is built; running it needs the
owner's drives. Until a row appears below, no Better Storage claim may name a
copy speed, a flush time, or a time to Ready to unplug.

### Method

One command per device, unprivileged, on the person's own desktop:

```sh
cargo run --release -p storage-platform --bin better-storage-bench -- \
    --mount /media/$USER/LABEL --medium usb-flash \
    --out engineering/performance/storage-runs.jsonl
```

`--medium` is `usb-flash`, `external-ssd`, or `external-hdd`. The harness
refuses `/` and any path that is not itself a mount point, writes only inside
a fresh hidden directory on that mount, removes it, and never mounts,
unmounts, or opens the block device. Each run is three workloads, three times
each by default (`--repeat`):

- `large-sequential`: one 1 GiB file
- `many-small`: 2,000 × 16 KiB files
- `metadata-tree`: depth 4, fan-out 6 (1,554 directories) with a 1 KiB file in each

Per run it records, as one JSON line (`storage_platform::bench::RunRecord`,
format 1): the device source, filesystem, kernel, rotational flag, the medium
named, and

- `copy_ms`: every write returned (application-visible completion, which is
  where default desktop behaviour stops until Eject)
- `flush_ms`: one `syncfs(2)` on the mount, as the service issues after an
  operation
- `writer_scan_ms` and whether the scan was complete
- `ready_ms`: copy + flush + scan, the earliest the service could say ready
- `app_visible_mib_per_s` and `durable_mib_per_s`: the difference is the cost
  of Direct Removal against default behaviour for the same copy
- `cpu_ms`: user + system CPU of the run

The matrix the ticket asks for is exFAT, NTFS, and ext4 on a USB flash drive,
an external SSD, and an external HDD: nine device and filesystem pairs.

### Not covered by the harness

- **Disconnect cleanup latency** needs a physical unplug; the harness cannot
  do one. Measure it from the service's own state updates when a stick is
  pulled while idle.
- **Throughput against default system behaviour** is the app-visible against
  durable comparison above. Default behaviour flushes at Eject, which unmounts
  and is not something this harness does.
- Page-cache warmth: the source is in memory, so these are device numbers,
  not a copy from another disk.

### Results

None yet. Paste the Markdown rows the command prints here, with the date and
the device model.
