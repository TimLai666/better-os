# ADR 0024: The installed-artifact record selects the rollback target

## Status

Accepted on 2026-08-02 (ticket 17). Written as a record on 2026-11-15 from the
delivery status decision log.

## Decision

The privileged daemon keeps a durable installed-artifact record per component,
written after each successful apply. A rollback uses it only when its version
matches what dpkg reports and the artifact is still cached. The artifact being
installed is never a fallback for the old version. A rollback counts as done
only when dpkg reports the expected result.

## Why

A transaction's rollback record describes one prior transaction; only a
version-matched component record proves which cached artifact produced the
version dpkg holds. Deriving the target from the new artifact made a failed
update reinstall the version that had just failed and report the host restored.

## Options rejected

- Deriving the previous artifact from the plan being applied: the defect above.
- Trusting APT's exit code for a rollback: does not prove dpkg's state.

## Consequences

- A component with no matching record cannot be rolled back to an artifact, and
  says so.
- The rule is described in `/engineering/specs/security-and-rollback.md`.
