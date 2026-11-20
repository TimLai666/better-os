# Ticket 252 — COSMIC Settings out of sight in the session

**Spec:** `design/system-settings-spec.md` decision M7; ADR 0042 (proposed)
**Blocked by:** 232, 233, 234, 235, 236, 237, 238, the owner's decision M7, and the owner's decision M1

## Why

Once wave 1 is built, COSMIC Settings in the launcher offers a second place
for the same settings and its Panel, Dock, and Workspaces pages for programs
the session does not run (F3).

## What to build

- M7 (b): the session package ships `com.system76.CosmicSettings.desktop`
  with `NoDisplay=true` in its own `XDG_DATA_DIRS` directory; link cards still
  launch COSMIC Settings' pages.
- M7 (c), after wave 2: read whether anything in the closure depends on
  `cosmic-settings`, and if nothing does, remove it from the closure the
  daemon compiles in (ticket 204), with the release that carries it.

## Done when

- [ ] Better Launcher in the session does not list COSMIC Settings, and a
      link card still opens its page; GNOME's launcher is unaffected.
- [ ] Screenshots in `/home/tim/Developer/better-os-evidence/252/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
