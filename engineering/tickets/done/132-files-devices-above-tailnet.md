# Ticket 132 — Better Files: Devices section above the tailnet

**Blocked by:** none

## Why

The owner asked on 2026-11-16 for the sidebar's Devices section (drives) to
sit above the Tailscale machines.

## What to build

- Reorder the sidebar so Devices comes before the tailnet machines, keeping
  every other section's order; the order lives in one place (`sidebar.rs`
  section order) and the keyboard navigation and collapse state follow it.

## Done when

- A test asserts the section order; screenshots at 1600 and 960.

## Notes

- The pane draws more groups than `SidebarSection`'s four (Dropbox, plugin
  sections, the tailnet), so the order is `sidebar::NAV_ORDER` over a
  `NavGroup` enum, and `FilesApp::nav_targets` walks it. Devices moved up one
  place, directly above the tailnet; nothing else moved. The spec's order is
  in `engineering/specs/files-gui-policy.md` ("The sidebar's order").
- Keyboard navigation (`nav_step`) and the narrow window's icon-only pane
  both follow the drawn lines, so they follow the order with no change of
  their own. The sidebar has no per-section collapse state to move.
- Screenshots (headless GNOME Shell 46 sandbox, fake `tailscale` on `PATH`,
  fixture drives) are outside the repository, in
  `better-os-evidence/132/`: 1600 wide in English and Chinese, 960 wide
  (icons) in both, and 960 with the pane opened.
