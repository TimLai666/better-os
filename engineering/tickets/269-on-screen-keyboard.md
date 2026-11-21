# Ticket 269 — An on-screen keyboard drawn by the login and lock screens themselves

**Spec:** `design/login-screen-spec.md` section 3.11
**Blocked by:** 260

## Why

Ubuntu 24.04's archive has no `squeekboard` (`apt-cache policy` on
2026-11-21), and before login nothing else can type into the greeter; a
keyboard drawn by the program itself needs no virtual keyboard protocol.

## What to build

- In `better-ui`: a keyboard view (a GPUI-free key layout model plus a thin
  view) for the XKB layouts `login-screen-core` names, with Shift, Caps,
  Backspace, Enter, the layout key, and keys that feed a text field
  directly; light, dark and high contrast; every key named for the screen
  reader.
- Shown across the bottom 40% of the primary output when the switch is on;
  the card moves up (wide) or the entry rises (narrow) to stay visible.

## Done when

- [ ] Model tests for the US layout and every layout the spec's indicator
      can show; screenshots of both appearances in
      `/home/tim/Developer/better-os-evidence/269/`.
