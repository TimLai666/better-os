# Ticket 237 — Date and time in Better Manager

**Spec:** `design/system-settings-spec.md` section 11; ADR 0042 (proposed)
**Blocked by:** 231, 206, the owner's decisions M4 and M6, and the owner's decision M1

## Why

Time zone, automatic time, and the clock format are COSMIC Settings' in the
session and GNOME Settings' under GNOME. The clock format is what Better OS's
own bar shows (ticket 160 reads `com.system76.CosmicAppletTime` in the
session).

## What to build

- `datetime-core` and `datetime-platform` (section 3.1): timedated's
  properties and methods with `interactive = true`, the time-zone list with
  offsets and the zh-TW city names of the copy table, the clock format per
  desktop, shared with the bar's clock (ticket 160).
- The page of section 11, the manual dialog with a date picker beside ticket
  87's time picker, automatic time zone under GNOME
  (`org.gnome.desktop.datetime automatic-timezone`) and its session state.
- Every polkit outcome as section 3.3 and ADR 0042 say.

## Done when

- [ ] Tests against a fake timedated: NTP on and off, `CanNTP` false, set time
      refused, cancelled, allowed; time-zone search in both languages; the bar
      and the page reading the same clock format.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/237/`. An account outside `sudo` is asked for an administrator's password.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
