# Ticket 62 — The declared Rust version builds the workspace, and CI runs on current actions

**Epic:** Build and release · **User Story:** a contributor who installs the
Rust version the workspace declares can build it · **Branch:** `ticket-60-62` ·
**Blocked by:** none · **Status:** ready

## What it delivers

The workspace declares `rust-version = "1.85"` and the locked dependencies need
at least 1.92 (`oo7 0.6.0`). CI uses stable Rust, so nothing notices. The
workflow's `actions/checkout@v4` and `actions/upload-artifact@v4` run on the
deprecated Node.js 20 runtime.

## Acceptance criteria

- [ ] `rust-version` equals the highest `rust_version` among locked packages,
      and every document that states the baseline says the same.
- [ ] CI gains a job or step that builds the workspace with exactly that
      toolchain, so the declaration cannot drift again.
- [ ] Each action in `.github/workflows/` is on its current major version that
      runs on Node.js 24, checked against the action's own releases.
