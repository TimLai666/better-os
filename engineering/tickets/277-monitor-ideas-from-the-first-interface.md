# Ticket 277 — Ideas from Better Monitor's first interface, for the owner to pick

**Blocked by:** the owner's choice of which to build

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks. These are smaller, and no specification asks for them.

## The list

- CPU affinity for a process (Windows Task Manager's "Set affinity"); only
  in `monitor-actions-linux`.
- The Wi-Fi network's name on the network page, read from NetworkManager over
  D-Bus (never by running `nmcli`).
- Redrawing less often while the window is not the active one: measure the
  cost first.
- The highest value since boot for each disk, network card and the battery
  (today the chart's last 60 seconds; the service keeps six hours).
- Several search terms separated by `|`.
- Selecting several processes and acting on all of them.
- A pseudo-long-strings test that doubles every string's length, beside the
  existing tests that measure both real languages at 960 wide.
- Units: binary or decimal sizes, Celsius or Fahrenheit, bits or bytes.

## Done when

- The owner has picked; each picked item is its own ticket, and the rest are
  recorded here as not wanted.
