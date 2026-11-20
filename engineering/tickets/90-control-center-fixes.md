# Ticket 90 — Control center: Fixes (Better Patch)

**Blocked by:** 84

## Why

Ticket 76 direction 5 proposed Better Patch: a list of known Linux desktop
faults, each able to detect itself, say what is wrong in plain words, fix,
check the fix, and undo it. ADR 0017 decides it is a page of the control
center with a GPUI-free engine and typed entries.

## What to build

Spec section 9.

- **`patch-core`** (new crate, no GPUI, no I/O): the entry trait (detect,
  describe, plan the change, apply, check, undo), entry states (not present,
  found, fixed and checked, applied but still present, undone, cannot check),
  and the record of what was applied so undo removes exactly that.
- **`patch-platform`** (new crate): the I/O each entry needs, behind seams
  tests replace. No shell, no fetched or supplied script, no root.
- **Entry 1, blank text from a damaged fontconfig cache.** Detection: ask
  fontconfig for several different families and styles and treat "every
  answer is the same file with an unknown style" as the fault, as observed in
  ticket 76. Fix: rebuild the user's cache with `fc-cache -f` run as a
  typed command with fixed arguments. Check: the same queries now give
  different answers. No undo needed.
- **Entry 2, half-size pointer over an Electron app.** Detection: a Flatpak
  app using an Electron runtime on a native Wayland session with a display
  scale above 1. Fix: a per-user override for that app only,
  `--nosocket=fallback-x11 --socket=x11` and
  `ELECTRON_OZONE_PLATFORM_HINT=x11`, written to the user's Flatpak override
  file for that app ID. Undo removes exactly those keys. Ticket 76's limits
  hold: verified on a Flatpak only; the entry says so and offers nothing for a
  non-Flatpak app.
- **Page** in `manager-gui/src/pages/fixes/`: found, applied, and all-known
  groups, Check again, the per-entry Fix and Undo with their results.
  Checking runs off the UI thread when the page opens and on Check again.
- Register search entries, the nav count, and the Home item.

## Acceptance

- [x] Each entry has tests for detection (present and absent), fix, check,
      and undo against fixture directories in a scratch HOME.
- [x] Nothing in `patch-*` spawns a shell or takes a command from data; a
      test asserts it over the crates' source, as `touchpad-gui` does.
- [x] Light and dark, zh-TW and English, 1600 and 960, screenshotted with a
      fixture fault present and with none.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## Notes

Delivered (spec: `engineering/specs/better-patch.md`):
- `patch-core` (no GPUI, no I/O): the `Entry` trait, `EntryState`, the
  record, the engine, both entries. `patch-platform`: the host, the Flatpak
  reader and writer, Mutter's scale, the closed command runner, the record
  store, a fixture machine for demo windows and sandboxes, and the removal
  step below.
- The page in `manager-gui/src/pages/fixes/` with search entries, the pane
  count, and Home's item (an undone fault that came back counts again). The
  debug-only placeholder is gone, and with it the `Placeholder` page status,
  so ticket 91's release gate holds by construction. Header buttons use the
  kit's own `SettingsExpander` header handling.
- Removing Better Manager undoes every applied fix that has an undo
  (owner's rule via the coordinator, 2026-11-21): the package ships
  `/usr/lib/better-os/before-removal/better-manager`, which its prerm runs as
  each person on removal, its postinst runs with `--undo` on abort-remove,
  and its postrm cleans up after (`packaging/maintainer/before-removal-accounts.sh`,
  which shares `accounts.sh` with the autostart postrm). A key the person
  changed after the fix is left and named (exit 3). The record is deleted,
  and the rebuilt font cache is left. Tests: `patch_platform::removal` (7),
  `packaging/tests/before-removal-accounts.test.sh` (11 checks, now in CI),
  and a `verify-deb.sh` check. Not run through a real `apt remove`.
- Checks run on the merge of `main` (2026-11-21):
  - `cargo test --no-fail-fast -p patch-core -p patch-platform -p manager-gui`: exit 0, 374 + 2 + 5 + 56 + 16 + 8 + 4 passed.
  - `cargo +stable clippy` and `cargo +1.99.0 clippy -p patch-core -p patch-platform -p manager-gui --all-targets -- -D warnings`: exit 0.
  - `cargo +1.95 check -p patch-core -p patch-platform -p manager-gui`: exit 0.
  - `cargo fmt --all --check`: exit 0.
  - License check: exit 0, after regenerating it.
  - `bash packaging/tests/autostart-overrides.test.sh` and `before-removal-accounts.test.sh`: exit 0.
  - shellcheck is not installed on this machine; CI runs it.
- Screenshots: `/home/tim/Developer/better-os-evidence/90/`. They were taken in
  a private headless GNOME Shell 46 with a scratch HOME and fixture machines
  (`BETTER_OS_DEBUG_FIXES_ROOT`), in light and dark, zh-TW and English, at 1600
  and 960, with a fault present and with none. They also show expanded,
  after-fix, applied, after-undo, and Home states, one shot after the merge,
  and one with the real host code in the sandbox at scale 2 (read only, no
  Fix pressed).

Never run either fix against the person's real fontconfig cache or Flatpak
overrides from an agent session; use the scratch HOME.

## Candidate fault from the owner's machine (2026-11-13), not yet confirmed

On battery, after GNOME blanks the screen (no suspend involved), the panel
takes noticeably long to light up again. The eDP panel has PSR enabled
(`amdgpu ... eDP-1: PSR support 1 ... sink PSR ver 4`). The owner will try
`amdgpu.dcdebugmask=0x10` (PSR off) on the kernel command line for a day or
two; if that fixes it, it becomes an entry here (detect amdgpu with a PSR
panel, apply and restore the parameter through the daemon), with its power
cost stated. A separate observation from the same resume:
`ucsi_acpi USBC000:00: failed to re-enable notifications (-110)` ten seconds
after an actual suspend exit.

## Candidate fault: the USB-C charging port stops responding (owner, 2026-11-13)

The owner reports the USB-C charging port sometimes stops working. The kernel
log on that machine shows the UCSI firmware interface timing out:
`ucsi_acpi USBC000:00: ucsi_handle_connector_change: GET_CONNECTOR_STATUS
failed (-110)` (2026-11-12 06:04) and `failed to re-enable notifications
(-110)` after a resume (2026-11-13 00:08). BIOS M5406WA.314 (2025-04-17),
kernel 7.0.0-42. When UCSI stops answering, Linux stops learning about plug
and unplug on that connector. Still to establish: whether charging itself
stops (power delivery is negotiated by the embedded controller, not by Linux)
or only what Linux reports. Candidate entry: detect the -110 errors, tell the
person in plain words, and offer to reset the interface (unload and reload
`ucsi_acpi` through the daemon), plus point to a newer BIOS from ASUS when one
exists. Not built until the symptom is confirmed.
