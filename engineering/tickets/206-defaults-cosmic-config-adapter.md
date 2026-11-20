# Ticket 206 — Better Defaults previews, applies, verifies, and restores COSMIC configuration

**Spec:** better-os-session.md S5 (Better Defaults row)
**Blocked by:** none

## Why

In the session, the keys Better OS sets (launcher binding, touchpad, window
attention's default, the carried-over appearance) are COSMIC configuration,
not GSettings: one RON file per key under `~/.config/cosmic/<id>/v<N>/`,
defaults under `/usr/share/cosmic/`, watched by every COSMIC process
(`libcosmic` `cosmic-config/src/lib.rs`, ADR 0038). Better Defaults has no
adapter for it.

## What to build

- **A `cosmic-config` adapter** in `defaults-platform` with the same preview,
  capture, apply, verify, and restore steps as the dconf adapter: read the
  effective value (the person's file, else the system default), rewrite one
  RON value keeping the rest of the file byte for byte, write atomically,
  read back; restore to the captured value, or delete the person's file when
  the capture was "no file" so the system default applies again.
- **Typed values**: the RON shapes the session's declarations need (string,
  bool, number, the shortcut and `system_actions` maps, `xkb_config`), each a
  closed type with no command variant beyond the `system_actions` value, which
  is checked against a fixed table of Better OS programs.
- **Manifest contract**: the adapter id, its kinds, and `sessions:
  [betteros]` in `engineering/contracts/component-manifest.md` and the
  every-integration-kind fixture; a released manager that does not know the
  adapter refuses the manifest, so the change says how the declarations ship
  only after a manager that knows it.
- The changed-externally rule ticket 94 uses: a value the person changed by
  hand is not overwritten without asking.

## Done when

- [ ] Unit tests against recorded config trees: each value type, a file with
      comments and other fields kept, a missing file, a newer `v<N>`
      directory, restore to "no file", the changed-externally hold.
- [ ] ADR 0009 gains the adapter in its table by amendment note at the top.
- [ ] fmt, clippy on stable, and the crates' tests pass.
