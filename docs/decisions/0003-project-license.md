# ADR 0003: GPL-3.0-or-later Root License

## Status

Accepted.

## Context

The initial scaffold deferred the root project license while the workspace and
release packaging contracts were being established. Distribution of the first
`.deb` release requires an explicit license and a review of third-party license
notices.

The current GPUI dependency chain includes `zlog` and `ztracing`, which are
licensed under GPL-3.0-or-later. Other dependencies include MPL-2.0 and
dual-licensed packages, so the release still needs a complete notice review.

## Decision

License Better OS first-party code under **GPL-3.0-or-later**. The repository
includes the complete license text in `LICENSE`, and the Rust workspace and
member crates declare the SPDX identifier `GPL-3.0-or-later`.

## Consequences

- Source and binary distributions of first-party Better OS code follow the
  copyleft and corresponding-source obligations of GPL-3.0-or-later.
- Release packaging must include the GPL text and complete applicable
  third-party license notices.
- Package signing, public APT repositories, privileged IPC, and release
  channels remain separate decisions.
