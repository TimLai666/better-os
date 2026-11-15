# ADR 0023: Better Monitor reads /proc and /sys itself and keeps five value states

## Status

Accepted on 2026-08-30 (ticket 22). Written as a record on 2026-11-15 from the
delivery status decision log.

## Decision

- Collectors read `/proc` and `/sys` directly; `sysinfo` is not a dependency for
  the metrics of ticket 22.
- An observation is one of five states: unknown, unsupported, permission
  denied, stale, or a measured value (a zero is a measurement). It is not an
  `Option`.
- Every collector takes a `Roots` parameter, so no path is hardcoded.

## Why

The eight CPU time categories, PSI, vmstat paging counters, diskstats queue and
service time, and per-process cgroup paths are absent from a portable
abstraction or flattened by it, and a portable API reports zero where an
interface is missing. Collapsing the states would tell a person a machine is
idle when it is unobserved: PSI on a kernel without `CONFIG_PSI` and a
descriptor count on another user's process forced the distinction. `Roots` lets
tests drive the production read path against captured trees.

## Options rejected

- `sysinfo`: evaluated in `/engineering/specs/monitor-collector-sources.md`;
  loses the detail above and reports zero for missing data.

## Consequences

- Each collector has to record its source and interpretation
  (`/engineering/specs/monitor-collector-sources.md`).
- Whether `sysinfo` serves battery, component naming and disk identity is still
  open and listed in `AGENTS.md`.
