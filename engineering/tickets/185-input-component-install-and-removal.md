# Ticket 185 — The Better Input component: install, turn off, update, remove

**Blocked by:** 181, 183, 184, 186

## Why

AGENTS.md: installing, turning on, and removing are one action each, and nothing
the component installed outlives it. Spec section 6 lists every step, including
the one IBus restart a new engine needs.

## What to build

- **Package `better-input`** built by `packaging/build-deb.sh`: the engine, the
  worker, the voice service and its user unit (with the maintainer scripts from
  `packaging/maintainer/user-units.sh`), the voice-key helper, its unit, account,
  and udev rule (ticket 188), the IBus component file, ONNX Runtime's library and
  notices, `THIRD-PARTY-LICENSES.md`; `verify-deb.sh` checks each, including the
  component file's exec path.
- **Manifest** `components/manifests/better-input.yaml` as spec section 6.4:
  `component_type: replacement`, `replaces: [ibus-chewing]`, `icon: generic`,
  `restart: none`, the `default_integrations` of section 6.2, health checks,
  permissions, `paths`; `personal_data::DATA_DIRECTORIES` gains the two
  person's-data entries; a second list for downloaded component content (the
  models) that removal always deletes, with tests that it never reaches another
  component's paths.
- **The IBus restart** (spec section 6.3): a controlled operation in
  `manager-platform` that try-restarts `org.freedesktop.IBus.session.GNOME.service`
  where it exists and runs `ibus restart` otherwise, run as the person after the
  package lands and after it is removed, kept off the person's own session in
  tests by `BETTER_OS_TEST_SYSTEMCTL_GUARD`; the health check
  `input.engine_registered` that the engine is in IBus's list.
- **Install order** (spec section 6.1): package, unit, IBus restart, Defaults
  entries only when ticket 181's gate allows (otherwise the page's notice), model
  downloads started; **turn off**, **turn on**, **remove** as the table says;
  the removal hook restores the Defaults entries before the package goes.
- **The install confirmation** shows the package and the models with the total
  (decision D4); a metered connection defers the download.

## Done when

- [ ] Unit tests for the manifest (its validation and that its health checks
      are the IDs the code emits), the two data lists, and the install, turn off,
      turn on, and remove sequences against fakes.
- [ ] In a disposable VM per release (22.04 and 24.04) with a GNOME session,
      installed from the built package through Better Manager: Better Input is
      the input source without a login and types; turning it off and removing it
      put back exactly the captured `sources` and `mru-sources`, 新酷音 types
      again, no Better Input file, unit, udev rule, or model is left, and the
      person's words are deleted or kept by the check box. How 22.04 restarts
      IBus is recorded.
- [ ] After release, on the owner's desktop: install, type, dictate, remove;
      recorded in Notes.
- [ ] fmt, clippy on stable, the crates' tests, `verify-deb.sh`, and the license
      inventory check pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

The IBus restart and the GNOME VM check are GNOME's. In the session the install order is the same with the session's framework in place of the IBus restart (Fcitx 5's own reload, or none under S4 (a)), and `replaces` means 新酷音 in Fcitx 5; ticket 221 builds and checks that half, so this ticket is unchanged.
