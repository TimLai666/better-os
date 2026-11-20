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

- **Engine and entries.** `patch-core` (no GPUI, no I/O) holds the `Entry`
  trait, `EntryState`, the record, the engine, and both entries.
  `patch-platform` holds the host, the Flatpak reader and in-place writer,
  Mutter's scale (5 s timeout; "cannot check on this desktop yet" off
  GNOME), the closed command runner, the record store, the location file, a
  fixture machine, and the removal step.
- **Entry 2 follows every Electron version.** Electron 38 removed
  `ELECTRON_OZONE_PLATFORM_HINT` (breaking-changes.md, 38.0). The override
  now also takes the `wayland` socket away and sets `XDG_SESSION_TYPE=x11`,
  and detection reads the app's Electron `version` file. The check looks at
  what the app will start with, not only at the keys written.
- **Where the fix is offered.** Only at one whole-number scale above 1. A
  fractional scale, or screens at different scales, is listed with the
  reason and no Fix.
- **Override files.** Edited in place: only the fix's own lines change.
  Line endings, spacing, repeated keys and repeated groups (read as GLib
  reads them) and the file's permissions are kept.
- **The page.** `manager-gui/src/pages/fixes/` has the search entries, the
  pane count and Home's item. The debug-only placeholder is gone,
  `render::page_rows` matches every pane item with no catch-all, and
  `every_page_in_the_pane_has_real_content` replaces the old gate test. The
  window stops offering changes while its package is being removed (the
  shared `UpdateState::Removed` in `better_core::self_update`). The demo
  window's fixture machine lives only under a private runtime directory.
- **Removing Better Manager undoes every applied fix that has an undo**
  (AGENTS.md: nothing a component adds outlives it):
  - The prerm runs the before-removal program as each person `getent
    passwd` lists in the login range. Each step runs under `timeout -s KILL
    30`, in its own session, with no terminal input.
  - A key changed after the fix is left and named. A fix whose undo failed
    stays recorded with its file named. The record is deleted otherwise.
  - `--undo` acts only on this dpkg run's stamped set-aside file.
  - The postrm cleans up what the program set aside. A removal that failed
    after the prerm is reported on the Fixes page at the next start.
  - The location file lets the removal find a record and overrides kept
    under custom XDG directories.
  - An unmounted encrypted home is the limit that remains.
- **Manifest.** `better-manager.yaml` `paths` lists the record, the
  set-aside file, the location file, and the before-removal program.

Checks run on the branch after merging `main` (2026-11-21):

- `cargo test --no-fail-fast -p patch-core -p patch-platform -p better-core
  -p better-ui -p manager-gui -p manager-core`: exit 0, all passed
  (manager-gui 378).
- `cargo +stable clippy` and `cargo +1.99.0 clippy` (`-p patch-core -p
  patch-platform -p better-core -p better-ui -p manager-gui --all-targets --
  -D warnings`): exit 0.
- `cargo +1.95 check` for the same crates: exit 0.
- `cargo fmt --all --check`: exit 0.
- The license check: exit 0, after regenerating it.
- `bash packaging/tests/autostart-overrides.test.sh` and
  `before-removal-accounts.test.sh`: exit 0. They include a hung home and a
  hung program being killed after their time limit.
- `sh -n` and `bash -n` on every changed script. `build-deb.sh`'s script
  writers were run against a scratch staging directory, and their output
  passes `verify-deb.sh`'s new checks.

Not run here: shellcheck (not installed on this machine), a real
`build-deb.sh`/`verify-deb.sh`, and the container check in
`packaging/test-daemon-e2e.sh`. This machine has no usable Docker; the new
`apt-get remove better-manager` section's first run is CI's.

Screenshots: `/home/tim/Developer/better-os-evidence/90/`, taken in a private
headless GNOME Shell 46 with a scratch HOME and fixture machines. They cover
light and dark, zh-TW and English, 1600 and 960 wide, and for each: found,
found expanded, after Fix, none, applied, after Undo, not offered
(fractional scale), and Home's item.

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
