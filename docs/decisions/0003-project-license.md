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
dual-licensed packages, so the release needs a complete, repeatable notice
review rather than an assumption based on direct dependencies alone.

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

## Review record

The locked dependency graph is inventoried in
[`docs/third-party-licenses.md`](../third-party-licenses.md). The current
review records 10 `GPL-3.0-or-later` package records, 3 `MPL-2.0` package
records, 2 expressions offering `LGPL-2.1-or-later`, one expression offering
`GPL-2.0-only`, and the additional `NCSA` and `bzip2-1.0.6` notice-sensitive
records. The report keeps each upstream SPDX expression and source reference
without changing or collapsing license alternatives.

Two pinned Zed workspace packages have no package-level Cargo license field.
The report records that metadata gap and the upstream checkout's
`LICENSE-GPL` and `LICENSE-APACHE` files for follow-up against file-level
markings. The package verifier requires the root GPL text and the generated
inventory in every Debian package. Any Cargo.lock change requires regenerating
and reviewing the inventory before another release.
