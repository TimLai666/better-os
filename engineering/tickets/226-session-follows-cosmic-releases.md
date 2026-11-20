# Ticket 226 — The session takes each COSMIC release without breaking the machine

**Spec:** better-os-session.md S7
**Blocked by:** 204 (for the closure check and held-back updates); 223 for the compositor rebase step

## Why

COSMIC made twelve releases in eleven months and changed behaviour Better OS
relies on without notice (the decoration default on 2026-11-06, ADR 0038).
A release that needs a package outside the pinned closure would leave APT
unable to upgrade the session, and a rebased gesture patch has to pass before
the session takes a new Epoch (ADR 0039).

## What to build

- **The closure check** `packaging/tests/session-closure.sh` (shell is
  allowed for packaging tooling): reads System76's `noble` `Packages` and the
  base's package lists from fixtures or a fetched snapshot, resolves the
  session's closure, and fails naming each package outside the 14 (or 13 once
  ticket 223 ships) and each versioned dependency the base cannot meet; run
  before each Better OS release and on demand, with fixture tests.
- **Held-back updates in Better Manager**: when APT holds a session package
  back, Updates shows 「COSMIC {version} 需要 Better OS 尚未支援的套件，先不更新。」 /
  "COSMIC {version} needs packages Better OS doesn't support yet, so it isn't
  updated." and never installs a part of the closure alone; the health check
  reads the closure's versions as one set.
- **The routine**, written in `engineering/specs/release-packaging.md`: the
  closure check, the patch's rebase onto the Epoch tag, ticket 202's protocol
  tests and replayed-recording test on the rebuilt compositor, flow 1 in the
  VM, then release.

## Done when

- [ ] Fixture tests for the closure check: a new dependency, a versioned
      dependency the base cannot meet, a renamed package.
- [ ] Unit tests for the held-back state and its copy in both languages.
- [ ] The routine run once for a real Epoch release, recorded in Notes.
