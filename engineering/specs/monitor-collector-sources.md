# Better Monitor collector source traceability

Issue #16 requires that every production collector record where its data comes
from, how that source was adopted, where Better Monitor's interpretation
differs from the source, and which tests prove that interpretation. This file
holds that record for the Linux collectors delivered in ticket 22.

## How the upstream specification was pinned

The kernel documentation was read from `docs.kernel.org`, which reports itself
as **kernel version 7.2.0** on the pages consulted. Field semantics were then
checked against the interfaces of the machine the fixtures were captured from:
**Linux 7.0.0-30-generic**, x86-64, AMD CPU with `k10temp`, `amd_pstate`
cpufreq, one NVMe disk, and a MediaTek wireless interface.

Fixtures are captured `/proc` and `/sys` trees rather than hand-typed samples,
so the recorded interpretation is checked against bytes a real kernel wrote.
The only edits made to a captured tree were replacing one process command line
that contained a local path, and removing `/proc/1/fd`, which an unprivileged
capture could not enumerate.

## Dependency evaluation: `sysinfo`

- **Adoption mode:** evaluated, not adopted for this ticket's scope.
- **Reason:** every metric in scope is either absent from a portable
  abstraction or flattened by it — the eight kernel CPU time categories, PSI,
  `/proc/vmstat` paging counters, `/proc/diskstats` queue and service time,
  per-process cgroup path and descriptor count. More importantly, a portable
  API has no equivalent of the five-way observation state this crate is built
  on: it reports zero where an interface is missing, which is the exact
  confusion the contract exists to prevent. Running both would also mean two
  sampling cadences over the same counters.
- **Still open:** battery, component temperature naming, and disk identity are
  plausible places to revisit it. Adopting it would be a new dependency and
  needs a recorded decision.

No code was copied or adapted from GNOME Resources, Mission Center, or
`bottom`. Their behaviour informed which metrics are worth collecting; no file,
asset, or implementation was reused, so no attribution obligation arises from
this ticket.

## `linux.cpu`

| | |
| --- | --- |
| Feature ID | `linux.cpu` |
| Upstream spec | `Documentation/filesystems/proc.rst` (`/proc/stat`, `/proc/loadavg`); `Documentation/admin-guide/pm/cpufreq.rst`; `Documentation/hwmon/sysfs-interface.rst` |
| Version pinned | docs.kernel.org 7.2.0; verified against Linux 7.0.0-30-generic |
| Adoption mode | original implementation against the platform interface |
| Licence | none required; no third-party code used |

Semantic differences and decisions:

- **Guest time is not double counted.** The kernel adds guest ticks into
  `user` and guest-nice ticks into `nice`. The reported `cpu.utilization.user`
  subtracts guest, matching what procps does, and the total omits the guest
  columns entirely. Reporting the raw fields would overstate user time and
  understate every other ratio on a machine running virtual machines.
- **`iowait` is neither idle nor busy.** `cpu.utilization.busy` excludes both
  `idle` and `iowait`, so waiting on a disk does not read as work done.
- **Counters saturate rather than wrap.** A CPU hot-unplugged and replugged can
  make a per-CPU counter appear to go backwards; the delta clamps at zero
  instead of producing a spike of near-2^64.
- **USER_HZ is taken as 100.** That is the `/proc` ABI constant for userspace,
  independent of the kernel's internal `CONFIG_HZ`.
- **cpufreq reports kilohertz**; the metric promises hertz and converts.
- **Per-core temperature is Intel-only in practice.** Only `coretemp` publishes
  `Core N` labels. On the AMD test machine `k10temp` publishes `Tctl` alone, so
  `cpu.temperature` is `Unsupported(NotReported)` per CPU while
  `cpu.package.temperature` has a value.
- **Known limitation:** `Core N` labels are matched to `topology/core_id`
  without resolving which package the hwmon device belongs to. A multi-socket
  machine with repeated core numbering would collide. Single-socket desktops
  and laptops are the supported target.

Tests proving the interpretation: `guest_time_is_not_counted_twice_because_the_
kernel_folds_it_into_user`, `waiting_on_io_is_neither_idle_nor_busy`,
`a_counter_that_went_backwards_saturates_instead_of_wrapping`,
`an_old_kernel_that_stops_at_idle_still_parses`,
`two_samples_produce_the_utilization_the_tick_deltas_imply`,
`cpufreq_kilohertz_are_converted_to_hertz`,
`an_amd_host_reports_a_package_sensor_and_no_per_core_temperature`, plus the
truncated and malformed cases.

## `linux.memory`

| | |
| --- | --- |
| Feature ID | `linux.memory` |
| Upstream spec | `Documentation/filesystems/proc.rst` (`/proc/meminfo`, `/proc/vmstat`) |
| Version pinned | docs.kernel.org 7.2.0; verified against Linux 7.0.0-30-generic |
| Adoption mode | original implementation against the platform interface |

Semantic differences and decisions:

- **`kB` in `/proc/meminfo` means kibibytes.** Every size is multiplied by 1024
  before it leaves the collector.
- **`pgpgin` and `pgpgout` count kibibytes, not pages,** despite the `pg`
  prefix. They are converted to bytes per second. `pswpin` and `pswpout` really
  do count pages and stay counts per second, because turning them into bytes
  would require a page size this crate does not read.
- **`memory.used` is `MemTotal - MemAvailable`,** not `MemTotal - MemFree`. The
  difference is the reclaimable page cache, which the user should not be told
  is used.
- **A machine with no swap** gets `Unsupported` for swap utilization rather
  than a utilization of zero, while `memory.swap.total` is a real zero.
- **A `/proc/vmstat` counter this kernel does not publish** is `Unsupported`,
  not a rate of zero.

Tests: `a_kibibyte_line_becomes_bytes_not_kilobytes`,
`used_memory_is_total_minus_available_not_total_minus_free`,
`page_in_counts_kibibytes_and_swap_in_counts_pages`,
`minor_faults_are_the_faults_that_were_not_major`,
`a_machine_with_no_swap_reports_unsupported_rather_than_zero_utilization`,
`a_vmstat_counter_this_kernel_lacks_is_unsupported_not_zero`,
`a_counter_that_did_not_move_reports_a_real_zero`, plus truncated and malformed
cases.

## `linux.pressure`

| | |
| --- | --- |
| Feature ID | `linux.pressure` |
| Upstream spec | `Documentation/accounting/psi.rst` |
| Version pinned | docs.kernel.org 7.2.0; verified against Linux 7.0.0-30-generic |
| Adoption mode | original implementation against the platform interface |

Semantic differences and decisions:

- **A kernel without `CONFIG_PSI` has no `/proc/pressure` at all.** The whole
  collector reports `Unsupported` once, with the missing path, and every metric
  on every resource says the same. Three files of zeroes would read as a
  perfectly healthy machine.
- **`avg10`, `avg60`, and `avg300` are already averaged by the kernel** over
  its own windows and are republished unchanged. The descriptor's sampling kind
  is `KernelAveraged` so a downstream consumer does not average them again.
- **The CPU file's `full` line is a compatibility zero.** The documentation
  says CPU `full` is undefined at system level and has been reported as zero
  since 5.13 for backward compatibility, so it is reported as `NotReported`
  rather than as a measurement. The `memory` and `io` `full` lines are real and
  are reported.
- **`total=` is microseconds,** and its delta over elapsed wall time gives the
  derived `pressure.*.stall_ratio`.

Tests: `a_kernel_without_config_psi_reports_unsupported_for_the_whole_
subsystem`, `the_kernels_averages_are_republished_rather_than_averaged_again`,
`the_cpu_full_line_is_not_offered_as_a_measurement`,
`the_stall_ratio_needs_two_samples`, plus truncated and malformed cases.

## `linux.process`

| | |
| --- | --- |
| Feature ID | `linux.process` |
| Upstream spec | `Documentation/filesystems/proc.rst` chapter 3 (`/proc/[pid]/stat`, `status`, `cmdline`, `cgroup`, `fd`) |
| Version pinned | docs.kernel.org 7.2.0; verified against Linux 7.0.0-30-generic |
| Adoption mode | original implementation against the platform interface |

Semantic differences and decisions:

- **The `comm` field can contain spaces and parentheses.** The line is split at
  the last `)`, not on whitespace, or every field after it shifts.
- **PIDs are reused.** The CPU-time delta is only used when the process start
  time is unchanged; otherwise the reading is `Unknown(NotYetSampled)` rather
  than a fabricated spike.
- **`process.cpu.utilization` is a fraction of one logical CPU,** so a
  multi-threaded process can exceed 1.0. Dividing by the CPU count would hide
  which process is saturating a core.
- **`/proc/[pid]/fd` is unreadable for another user's processes,** which is
  `PermissionDenied`, never a count of zero.
- **Command lines are withheld by default,** reported as
  `Unsupported(PolicyWithheld)` so the reason is visible and distinguishable
  from a kernel that said nothing. Issue #16 requires no persistent
  command-line capture by default.
- **A kernel thread has no `VmRSS`,** which is `Unsupported(NotReported)`, not
  zero memory.
- **A truncated `status` line leaves that one field unknown** while the rest of
  the file still parses; a value that is present but not a number is malformed.
- **cgroup v2's `0::` line wins** over any v1 hierarchy, with the first v1 path
  as a fallback.
- **Storage throughput comes from `read_bytes`/`write_bytes`, not
  `rchar`/`wchar`.** Added in ticket 23 so the Apps view can attribute disk
  activity. `rchar` and `wchar` count every read and write including cache hits
  and pipes, so a process that only talked to a socket would look like it was
  hammering the disk. `/proc/[pid]/io` is readable only for a process the
  caller may ptrace, so for another user's process the counters and the derived
  rates are `PermissionDenied`, never zero. The rate uses the same start-time
  guard as the CPU delta, so a recycled PID does not report a whole previous
  lifetime as one interval's throughput.
- **Not collected in this ticket:** per-process GPU and network attribution.
  Issue #16 defers both, and inferring them would be a guess presented as data.

Tests: `an_executable_name_with_spaces_and_parentheses_does_not_shift_every_
field`, `a_reused_pid_does_not_inherit_the_previous_process_cpu_time`,
`command_lines_are_withheld_unless_collection_is_configured_to_include_them`,
`an_unreadable_fd_directory_is_permission_denied_not_a_count_of_zero`,
`a_kernel_thread_without_vmrss_is_unsupported_rather_than_zero_memory`,
`a_cgroup_v2_line_wins_over_any_v1_hierarchy`,
`start_time_and_runtime_come_from_boot_time_and_uptime`,
`a_process_whose_cpu_time_did_not_move_reports_a_real_zero`,
`storage_byte_counters_come_from_the_block_layer_fields`,
`process_throughput_needs_two_rounds_and_divides_by_the_real_interval`,
`an_unreadable_io_file_is_not_reported_as_no_disk_activity`, plus truncated and
malformed cases, and the whole-set test
`no_command_line_reaches_a_report_under_the_default_privacy_setting` which runs
against the live host.

## `linux.storage`

| | |
| --- | --- |
| Feature ID | `linux.storage` |
| Upstream spec | `Documentation/admin-guide/iostats.rst` |
| Version pinned | docs.kernel.org 7.2.0 (17 counters after major, minor, and name); verified against Linux 7.0.0-30-generic |
| Adoption mode | original implementation against the platform interface |

Semantic differences and decisions:

- **A sector in this interface is 512 bytes,** always, regardless of the
  device's `queue/logical_block_size`. The documentation does not state the
  unit; this is the recorded interpretation, and the fixture proves the
  conversion.
- **Partitions are excluded.** `nvme0n1p6`'s traffic is already inside
  `nvme0n1`, so counting both would double the machine's apparent I/O. The
  filter is membership in `/sys/block`, which partitions are not part of.
- **`loop`, `ram`, `zram`, and `fd` devices are excluded** as file-backed
  virtual devices. `dm-` devices are kept: an encrypted or LVM volume is where
  a user's real I/O goes.
- **Discard counters need kernel 4.18 and flush counters need 5.5.** A kernel
  without them reports `Unsupported`, not zero discards.
- **Utilization is `io_ticks` delta over elapsed wall time** — what `iostat`
  calls `%util` — and the weighted time delta over elapsed time is the mean
  number of requests in flight, which is queue depth and not a duration.
- **An interval with no completed reads has no mean latency,** reported as
  `Unknown`, rather than a latency of zero.

Tests: `parses_all_seventeen_counters_of_a_captured_diskstats_line`,
`sectors_are_five_hundred_and_twelve_bytes_regardless_of_the_block_size`,
`partitions_and_loop_devices_are_left_out_of_the_report`,
`a_device_mapper_volume_is_treated_as_real_storage`,
`a_kernel_without_the_discard_counters_reports_them_as_absent`,
`utilization_and_queue_depth_come_from_the_two_time_counters`,
`latency_is_service_time_per_completed_request`,
`an_interval_with_no_completed_reads_has_no_latency_rather_than_zero`, plus
truncated and malformed cases.

## `linux.network`

| | |
| --- | --- |
| Feature ID | `linux.network` |
| Upstream spec | `Documentation/filesystems/proc.rst` (`/proc/net/dev`); `Documentation/ABI/testing/sysfs-class-net`; `include/uapi/linux/if_arp.h` for `ARPHRD_*` |
| Version pinned | docs.kernel.org 7.2.0; verified against Linux 7.0.0-30-generic |
| Adoption mode | original implementation against the platform interface |

Semantic differences and decisions:

- **`/proc/net/dev` pads the interface name into a fixed column.** A long name
  such as `tailscale0:` leaves no space before its first counter, so the line
  is split on the colon; a whitespace split folds the first value into the
  name. The captured fixture contains exactly this case.
- **`/sys/class/net` is the authority on which interfaces exist,** because it
  also carries link state. An interface present in only one of the two sources
  keeps whichever half is real and reports the other as unknown.
- **`speed` is often not a number.** A wireless or virtual link returns
  `EINVAL`, and a down Ethernet link returns `-1`. Both are
  `Unsupported(NotReported)`, never a speed of zero. Megabits are converted to
  bits per second.
- **Wireless interfaces report `ARPHRD_ETHER`.** They are separated from real
  Ethernet by the presence of `phy80211` or `wireless`, not by the type number.
- **`carrier` returns `EINVAL` while an interface is down,** which is
  `Unsupported`, not "no carrier".
- **Per-process network attribution is absent** by decision, per issue #16.

Tests: `parses_a_captured_net_dev_including_a_name_that_fills_its_column`,
`the_two_header_rows_are_not_mistaken_for_interfaces`,
`a_link_speed_in_megabits_becomes_bits_per_second`,
`a_link_with_no_speed_is_unsupported_rather_than_zero`,
`a_speed_of_minus_one_is_a_driver_saying_it_does_not_know`,
`a_wireless_interface_is_not_reported_as_ethernet`,
`every_sysfs_interface_appears_even_without_a_proc_line`,
`losing_sysfs_degrades_the_collector_but_keeps_the_counters`, plus truncated
and malformed cases.

## Measured overhead

Issue #16 forbids claiming low overhead without published measurements. These
are from `cargo run -p monitor-collectors-linux --release --example overhead --
100` on the reference machine described above, with 1,359 tasks running:

| | |
| --- | --- |
| Rounds | 100 |
| Total wall | 1.020 s |
| Mean round | 10.198 ms |
| Worst round | 12.209 ms |
| Process CPU consumed | 1.01 s, 99.0% of wall |

Per collector, mean and worst wall time for one round:

| Collector | Mean | Worst |
| --- | --- | --- |
| `linux.cpu` | 581 µs | 824 µs |
| `linux.memory` | 117 µs | 310 µs |
| `linux.pressure` | 23 µs | 60 µs |
| `linux.process` | 9.178 ms | 11.000 ms |
| `linux.storage` | 171 µs | 310 µs |
| `linux.network` | 128 µs | 185 µs |

What the numbers say. Collection is CPU-bound, not I/O-bound: `/proc` reads do
not touch a disk. The process table is 90% of the cost and scales with the task
count, so it is the collector an adaptive sampling policy has to slow down
first. Everything else together costs about 1 ms per round. At a one-second
cadence a full round is roughly 1% of one logical CPU, which on this 24-thread
machine is under 0.05% of the machine — but that figure belongs to this
hardware and this task count, and a machine with 10,000 processes has not been
measured. That case is on the issue's benchmark list and is not covered here.

The same measurement runs as a test in `tests/overhead.rs`, which asserts only
a loose 500 ms per-round ceiling so it does not become a hardware-specific
tripwire, and prints the numbers under `--nocapture`.

### Re-measured after ticket 23 added per-process I/O

Adding the `/proc/[pid]/io` read made the process collector more expensive, and
the honest thing is to publish the new number rather than leave the old one
standing. Same command, same machine, 493 processes:

| | Ticket 22 (1,359 tasks) | Ticket 23 (493 processes) |
| --- | --- | --- |
| Mean round | 10.198 ms | 15.052 ms |
| Worst round | 12.209 ms | 29.602 ms |
| `linux.process` mean | 9.178 ms | 13.637 ms |

The two runs are not a like-for-like comparison of task counts, but the
direction is clear and is explained by the interface rather than by the parser:
`/proc/[pid]/io` is gated by a ptrace-mode permission check, so every read pays
for that check and reads on another user's process pay for it and then fail.
That is the price of a disk column that is attributable at all. It is also the
strongest argument yet for the adaptive cadence the specification describes:
the process collector is the one to slow down first, and it is now 90% of the
round rather than 90% of a cheaper round.

## Application grouping (`monitor-views`)

| | |
| --- | --- |
| Feature ID | `views.grouping` |
| Upstream spec | systemd `systemd.special(7)` slice layout; `systemd.scope(5)`; freedesktop Desktop Entry launch behaviour; Flatpak and snapd scope naming as observed in captured `/proc/[pid]/cgroup` values |
| Version pinned | verified against Linux 7.0.0-30-generic with systemd user sessions; fixture `snapshot-a` carries a real `app-<AppID>-<random>.scope` path |
| Adoption mode | original implementation; GNOME Resources and Mission Center used as behaviour references only |
| Licence | none required; no third-party code used |

Semantic differences and decisions:

- **Grouping reads the cgroup string ticket 22 already collects.** Nothing
  executes systemd, opens a D-Bus connection, or scans `.desktop` files. A
  desktop-entry name resolver can be supplied later; the engine does not need
  one to be correct.
- **Executable identity never merges anything.** Issue #16 lists it last as
  evidence, and also forbids merging processes solely because their executable
  names match. Both hold here because the fallback key carries the PID: the
  executable name labels a group of one, and two unrelated `python3` processes
  stay two groups. Proven by
  `unrelated_processes_with_the_same_executable_name_are_never_merged`.
- **The owning user is part of every group key.** One unit run by two users is
  two applications; a single CPU total across both would misattribute the
  machine's load.
- **Precedence is configuration, not a decision.** The default is Issue #16's
  order, and ticket 23 records that the final precedence and confidence
  thresholds still need an ADR. `GroupingPrecedence` is a list, and
  `precedence_decides_which_identity_names_a_flatpak_scope` proves reordering
  changes the recorded evidence.
- **The label and the evidence are separate questions.** A Flatpak launched
  into a systemd scope is grouped by unit membership under the default order
  and still labelled GIMP, because the cgroup carries the application id. The
  evidence is shown next to the name, so the label never overstates what is
  known.
- **Ancestry is bounded.** The walk up the parent chain refuses to revisit a
  PID, so a malformed parent relationship cannot hang the view. Proven by
  `a_cycle_in_the_parent_chain_does_not_hang_the_grouping`.

## Process actions (`monitor-core`, `monitor-actions-linux`)

| | |
| --- | --- |
| Feature ID | `actions.process` |
| Upstream spec | `kill(2)`, `setpriority(2)`, `getpriority(2)`, `signal(7)` |
| Version pinned | glibc on Linux 7.0.0-30-generic |
| Adoption mode | direct syscall through the `libc` bindings crate |
| Licence | `libc` is MIT/Apache-2.0; no code copied |

Semantic differences and decisions:

- **The GUI never names a signal number.** `ProcessAction` is an intent and
  `SignalKind` is a closed set; the mapping to `SIGTERM`/`SIGKILL`/`SIGSTOP`/
  `SIGCONT` exists in exactly one function in `monitor-actions-linux`.
- **A delivered signal is not an exit.** The outcome is `SignalAccepted`, and
  the interface says the process decides when it ends.
- **Own-user processes only.** Cross-user and elevated actions are refused
  before anything is attempted, with the owner named. The narrow
  polkit-reviewed boundary for those is deliberately not built here, and
  `manager-daemon` is untouched.
- **Lowering a nice value needs `CAP_SYS_NICE`,** so it is refused rather than
  attempted and failed. Raising one is allowed and read back.
- **`init` and Better Monitor's own process are never signalled.**
- **The policy is checked again inside `perform`,** so a view that forgot to
  check cannot reach a syscall.

Tests: 14 policy tests in `monitor-core`, 6 in `monitor-actions-linux`, and 4
integration tests in `monitor-actions-linux/tests/real_process.rs` that spawn a
real child and assert against `/proc` that `SIGSTOP` stopped it, `SIGCONT`
resumed it, `SIGTERM` ended it, and a renice took effect.

## Large-process benchmark

Issue #16 requires 100, 1,000, and 10,000 process scenarios. The dataset is
synthetic, generated fresh each run, so the numbers do not depend on what
happens to be running. Run with `cargo bench -p monitor-views`. Mean of 20
iterations on the reference machine:

| Operation | 100 | 1,000 | 10,000 |
| --- | --- | --- | --- |
| Build model and sort | 0.034 ms | 0.118 ms | 2.057 ms |
| Re-sort by another column | 0.001 ms | 0.003 ms | 0.071 ms |
| Apply a filter | 0.016 ms | 0.041 ms | 0.482 ms |
| Build the tree view | 0.017 ms | 0.041 ms | 0.538 ms |
| Adopt a new round | 0.030 ms | 0.108 ms | 1.697 ms |
| Group into applications | 0.173 ms | 0.678 ms | 12.359 ms |
| Apps model with aggregates | 0.113 ms | 0.907 ms | 15.321 ms |

What the numbers say. The Processes table is not the problem at any size:
adopting a new round costs 1.7 ms at 10,000 processes, which is a tenth of a
frame, and scrolling costs nothing at all because the table is virtualized and
draws only the visible window. Application grouping is the expensive part, and
at 10,000 processes rebuilding the Apps model costs 15.3 ms — about one frame.
On a normal desktop that is 0.9 ms and irrelevant, but the 10,000-process case
is a known limit rather than a solved one: the grouping runs on the interface
thread when a round is adopted, and moving it to the sampling thread is the
obvious next step if that scenario becomes real. It is recorded here rather
than described as fine.

Not measured here: rendered frame time and dropped frames under scroll. That
needs a display server and a frame-timing harness, neither of which exists in
this workspace yet, so no frame-time claim is made.
