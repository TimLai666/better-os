# Ticket 302 — One reading of the XDG base directories

**Spec:** AGENTS.md ("A rule that two places need lives in one place both call")
**Blocked by:** none

## Why

The XDG base directory specification's rules (an unset or relative
`XDG_STATE_HOME` means `~/.local/state`; an unset or empty
`XDG_DATA_DIRS` means `/usr/local/share:/usr/share`, relative entries
ignored) are written out again in many crates, and the copies already
disagree: some take a relative `XDG_STATE_HOME`, some an empty one, some
keep relative `XDG_DATA_DIRS` entries, and two spell the default with
trailing slashes. Ticket 253's review found Better Screenshot about to add
two more; it uses `files_platform::xdg::data_dirs_from_env` and its
runtime directory instead.

Copies of the `XDG_STATE_HOME` rule (2026-11-22):

- `crates/awake-gui/src/settings.rs:230`
- `crates/awake-store/src/lib.rs:359`, `src/history.rs:505`, `src/rules.rs:209`
- `crates/defaults-core/src/removal.rs:117`
- `crates/files-gui/src/settings_store.rs:315`, `src/plugins.rs:128`
- `crates/git-gui/src/recent.rs:27`
- `crates/git-platform/src/trust.rs:44`
- `crates/launcher-gui/src/pins.rs:250`
- `crates/manager-store/src/lib.rs:390`, `src/catalog.rs:199`
- `crates/monitor-store/src/history.rs:255`
- `crates/patch-platform/src/paths.rs:37`
- `crates/touchpad-platform/src/removal.rs:66`

Copies of the `XDG_DATA_DIRS` rule:

- `crates/app-catalog-platform/src/discovery.rs:18` (`DEFAULT_DATA_DIRS`) and 59
- `crates/app-chooser-core/src/mime.rs:295`
- `crates/app-chooser-gui/src/chooser.rs:539`
- `crates/better-ui/src/lib.rs:490`
- `crates/defaults-platform/src/gsettings.rs:53`, `src/dconf_profile.rs:168`
- `crates/files-platform/src/xdg.rs:366` (`data_dirs_from_values`)
- `crates/manager-gui/src/window_attention_bus.rs:156`

## What to build

- One module in `better-core` (no new dependency) with `state_home`,
  `data_home`, and `data_dirs`, each over given values for tests and from
  the environment, following the specification: relative paths are
  ignored, an empty value is unset.
- Every place above calls it. Where a copy's behaviour differs from the
  specification, the change is named in the commit and its tests updated
  only with that reason.
- A test like `crates/better-core/tests/os_release_readers.rs` fails when a
  crate outside `better-core` reads `XDG_STATE_HOME` or `XDG_DATA_DIRS`
  outside its tests.

## Done when

- [ ] One implementation; no other crate reads either variable outside its
      tests, and the test above says so.
- [ ] fmt, clippy, and the tests of every crate touched pass.
