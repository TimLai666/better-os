# ADR 0001: Initial Boundaries

## Status

Accepted for the initial scaffold.

## Decision

Use one Rust workspace with independent core, manager, monitor, UI, CLI, and
GUI crates. Keep manager planning separate from privileged execution. Use
GitHub Releases with `.deb` artifacts as the initial distribution direction and
local APT installation as the future execution path.

## Why

The project needs one shared planning contract for CLI and GUI while preserving
a security boundary around system mutation. Independent manifests allow future
components to be versioned and validated without cloning code into system paths.

## Deferred

Root license, package signing, privileged IPC, public APT repository design,
release channels, and future per-component licensing remain undecided.
