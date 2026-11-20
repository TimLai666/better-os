# Ticket 211 — Alt+Tab switches windows in the session

**Spec:** better-os-session.md S4 (window switcher row)
**Blocked by:** 200, 203

## Why

COSMIC's window switcher is part of `cosmic-launcher`, which the session does
not install (to be confirmed by ticket 201's inventory; if 201 finds
`cosmic-comp` switches windows by itself, this ticket draws Better OS's
switcher over that and says so). A desktop without Alt+Tab is not a working
day.

## What to build

- **Crate `window-switcher`** (GPUI-free core: the order, most recent first
  per workspace or all workspaces by setting, grouping by application for
  Super+Tab, the selection rules) and a layer-shell view on the overlay layer,
  keyboard exclusive while shown, on the output with the focused window.
- **Sources**: `zcosmic_toplevel_info_v1` for windows, their states, and
  activation order; `zcosmic_toplevel_manager_v1` to activate; application
  names and icons from `app-catalog-core` by `app_id`.
- **Keys** in the session's defaults: Alt+Tab, Alt+Shift+Tab, Super+Tab;
  release of Alt activates; Escape cancels; mouse click activates.
- Accessible names and roles; zh-TW and English copy for the empty state
  (沒有開啟的視窗 / No open windows).
- A user unit in its package, enabled by `manager-core`, `WantedBy=better-os-session.target`.

## Done when

- [ ] Unit tests for ordering, grouping, cancellation, a window closing while
      the switcher is open.
- [ ] In the nested compositor or the VM with four windows of three
      applications: Alt+Tab, Super+Tab, Shift reverses, a window closed while
      shown; screenshots in `/home/tim/Developer/better-os-evidence/211/`.
- [ ] fmt, clippy on stable, the crates' tests, and `verify-deb.sh` pass.
