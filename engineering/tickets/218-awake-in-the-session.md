# Ticket 218 — Better Awake keeps the screen on in the session, and sees fullscreen windows

**Spec:** better-os-session.md S5 (Better Awake row)
**Blocked by:** 203

## Why

In the session, blanking is `cosmic-idle`'s, which honours only
`org.freedesktop.ScreenSaver.Inhibit`; logind locks still hold suspend and
the lid (ADR 0038). Better Awake's service already names a ScreenSaver backend
as Phase 4. A fullscreen window is visible in `zcosmic_toplevel_info_v1`,
which GNOME does not offer, so the fullscreen trigger ADR 0010 left
unavailable becomes possible in the session.

## What to build

- **The ScreenSaver backend** in `awake-platform` and `awake-service`:
  `Inhibit` and `UnInhibit` on `org.freedesktop.ScreenSaver` beside the logind
  inhibitor, acquired, verified, and released with it, used in the session (and
  wherever the name exists); the fake backend's tests extended.
- **The fullscreen provider** on `zcosmic_toplevel_info_v1`'s fullscreen
  state, three-valued as every provider (unknown where the protocol is
  absent, so GNOME keeps reporting it unavailable), with the rule editor's
  explanation updated.
- ADR 0010's provider list amended by a note at the top; AGENTS.md's Better
  Awake follow-up says the fullscreen trigger works in the session only.

## Done when

- [ ] Unit tests for the ScreenSaver backend's acquire, verify, lost, and
      release with a fake, and for the provider's three values.
- [ ] In the VM session: an Awake session keeps `cosmic-idle` from blanking
      past its timeout; a fullscreen video starts the fullscreen rule and
      leaving fullscreen ends it.
- [ ] fmt, clippy on stable, and the crates' tests pass.
