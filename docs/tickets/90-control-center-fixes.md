# Ticket 90 — Control center: Fixes (Better Patch)

**Blocked by:** 84 · **Status:** ready

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

- [ ] Each entry has tests for detection (present and absent), fix, check,
      and undo against fixture directories in a scratch HOME.
- [ ] Nothing in `patch-*` spawns a shell or takes a command from data; a
      test asserts it over the crates' source, as `touchpad-gui` does.
- [ ] Light and dark, zh-TW and English, 1600 and 960, screenshotted with a
      fixture fault present and with none.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## Notes

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
