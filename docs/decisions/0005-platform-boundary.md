# ADR 0005: Platform Boundary Crate

## Status

Accepted for Better Manager.

## Decision

`manager-platform` owns every interface that touches the host: system
capability reporting, artifact download, package application, and privileged
execution. `manager-core` depends on it for the system profile and never probes
the host itself.

The shipped implementations are:

- `MockPlatform` reports a supplied profile and confirms a declared checksum
  without fetching anything.
- `MockPlatform::apply` and `UnapprovedPrivilegedExecutor::execute` both return
  `PrivilegedExecutionNotApproved`.

No shipped code path applies a package change.

## Why

Issue #8 lists `manager-platform` as a crate boundary and says the privileged
executor stays an interface until its security design is approved. Keeping the
system profile inline in `manager-core` blurred that boundary: the planner
owned a description of the host it should have been given.

A package backend that returned a fake success would claim the host changed.
Mock lifecycle progress belongs to `manager-core`, which advances deterministic
state and is honest that it is doing so; the platform crate refuses instead.

## Deferred

The privileged daemon IPC protocol, its authentication flow, the real APT
backend, and real artifact downloading remain undecided. They are the
implementations these traits exist to receive.
