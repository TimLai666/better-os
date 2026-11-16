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
