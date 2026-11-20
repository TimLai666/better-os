# Ticket 184 — Better Defaults sets and restores the input source

**Blocked by:** none

## Why

Installing Better Input makes it the input source and removing it puts 新酷音
back (ticket 99), through Better Defaults' capture, apply, verify, and restore
(spec section 6.2). The `input-method` kind and adapter id exist in the schema,
but no production adapter does (ADR 0009), and GNOME's `sources` key is an
`a(ss)` value that `DefaultsValue` cannot hold. This is needed whichever engine
the owner chooses.

## What to build

- **A value type** `input_sources` in `better_core::defaults::DefaultsValue`: an
  ordered list of `(type, id)` pairs, each a lower-case token checked like other
  manifest values (`xkb` or `ibus`, id charset bounded), with the manifest
  contract (`engineering/contracts/component-manifest.md`) and the
  every-integration-kind fixture updated; a released manager that does not know
  the type refuses the manifest, so the change says how Better Input's manifest
  ships only after a manager that knows it.
- **The dconf encoding** of `a(ss)` in `defaults-platform`'s change-set encoder,
  pinned against GLib's bytes as the other types are.
- **The `input-method` adapter**: reads, applies, verifies, and restores
  `org.gnome.desktop.input-sources sources` and `mru-sources` as one change set,
  each key restored to its own captured value; the desired list is computed from
  the captured one (replace a named engine in place, or prepend when it is
  absent: spec section 6.2, decision D8).
- **Restore when the captured engine is missing**: reported as its own class
  (like `PreviousApplicationMissing`), the list written anyway.
- **The page**: the Default apps page lists the entry with the kind's existing
  label (輸入法 / Input method), in both languages.
- ADR 0009 gains the adapter in its table by amendment note at the top, as
  tickets 57, 58, 71, and 72 did.

## Done when

- [ ] Unit tests: the value type's validation and serialisation, the encoder
      against GLib's bytes, replace-in-place and prepend, restore to the exact
      captured value, a value changed by hand since capture not overwritten
      without confirmation, the missing-engine report.
- [ ] Against a real `dconf-service` on a private bus: apply, verify, restore,
      and the second read agreeing.
- [ ] fmt, clippy on stable, the crates' tests pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

This adapter writes GNOME's `org.gnome.desktop.input-sources` and stays GNOME's. In the session the input-source list is Fcitx 5's profile, written by the typed writer ticket 208 adds (with the same capture and restore), which ticket 221 uses to make Better Input the session's source; under S4 (a) the session's source is Better Input's own client instead. Nothing here changes.
