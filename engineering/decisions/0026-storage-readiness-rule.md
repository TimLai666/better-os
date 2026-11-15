# ADR 0026: The Ready to unplug rule and its two thresholds

## Status

Accepted on 2026-11-15 (ticket 31). Issue #5 asked for this choice to be
recorded rather than left in code. The rule itself shipped with the storage
crates; this record states why it is shaped the way it is and fixes the two
values that are policy rather than fact.

## Context

Better Storage tells a person a volume can be pulled out. That claim is only
worth what it rests on, and an unprivileged session process can read some
signals and not others
(`/engineering/specs/storage-safety-signals.md` lists each one with its status).
Two values in `storage_core::EvidencePolicy` decide how the rule treats the
gaps: whether a partial open-writer scan may support a claim, and how old a
verified flush may be.

## Decision

A mounted volume is **Ready to unplug** only when every one of these holds:

1. no tracked file operation (Better Files, a future Better Copy) is in flight
2. the open-writer scan produced an observation and found no writer
3. a `syncfs(2)` on the mount was verified, and no write was observed after it
4. per-device writeback, when readable, is zero
5. the verified flush is not older than `max_proof_age`

An unmounted volume needs only rule 1. Any missing, unreadable, or unsupported
signal produces **Unknown**, never a softer ready. `ReadyToUnplug` can only be
built from a `ReadinessProof`, which has no `Deserialize`, so the rule cannot
be bypassed by a client.

There is no numeric confidence score. Confidence is two-valued and explicit:
a proof records `fully_corroborated`, which is false when the writer scan was
partial or per-device writeback was unreadable. A weighted score would turn
"we could not see" into a number that reads as a fact, which AGENTS.md calls a
defect.

The two thresholds:

- **`require_complete_writer_scan = false`.** A session process cannot read
  other users' `/proc/<pid>/fd`. On the host this was run on, 365 processes
  were unreadable on an idle desktop. Requiring completeness would make every
  device Unknown on every normal machine for a reason unrelated to the device.
  The processes that cannot be seen belong to other users and system services;
  the person's own writers, which are the ones that matter for their own
  stick, are all visible. The gap is carried in the proof, not hidden.
- **`max_proof_age = 5 minutes`.** Observation is event-driven: a tracked
  write or a writeback change invalidates a proof at once. The bound exists
  only for an event the service missed (a writer it could not see, a restart
  of UDisks2). Five minutes is long enough that an idle stick does not flicker
  to Unknown and need a re-flush every few seconds, and short enough that a
  missed event cannot keep a stale green light for a whole session.

## Options rejected

- **A weighted confidence score with a threshold.** Hides which signal was
  missing and invites tuning a number nobody can justify.
- **Require a complete writer scan.** Correct in principle, permanently
  Unknown in practice without privilege (ADR 0027).
- **No proof age at all.** Relies on never missing an event.
- **Machine-wide `Dirty + Writeback` as a gate.** It says nothing about one
  device and would block readiness whenever any other disk is busy. It stays a
  corroborating heuristic, shown and never decisive.

## Consequences

- "Ready to unplug" means "no known pending writes", never physical
  persistence; a device cache or a lying USB bridge is out of reach (the
  safety signals spec says so in the words the UI must keep).
- Changing either threshold is a new record. Both are `EvidencePolicy` fields
  with tests, so a change is one line plus that record.
- Which filesystems and transports the claim is made for is declared in the
  manifest's `hardware_support` group, and the real-device measurements that
  would confirm the rule on hardware are still owed
  (`/engineering/performance/storage-performance.md`).
