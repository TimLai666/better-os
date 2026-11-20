# Ticket 227 — The owner works a week in the Better OS session on the laptop

**Spec:** better-os-session.md "Ready to become the default" item 5
**Blocked by:** 224, 225 (or the owner deferring the edge swipe), 226, and a release carrying them

## Why

ADR 0038 keeps GNOME the default until the session has passed its VM check
and a week on the owner's machine. Only real hardware shows what a VM with
software rendering cannot: the Radeon 890M, the touchpad under a hand, the
display at scale 2, Bluetooth, suspend and resume, the voice key.

## What to build

- With the owner, after the release: install the session from Better Manager
  on the laptop, log in, and use it as the daily desktop for seven days.
- A daily note in this ticket's Notes of what failed, with the journal lines;
  each fault fixed in its owning ticket or a new ticket, the ones that lose
  data or block work before the week counts.
- The gesture thresholds and `EventScale` tuned against the owner's hand
  (AGENTS.md: never tuned so far), recorded with the values.
- At the end, flow 3 on the laptop if the owner wants to go back, or a
  recorded "stays".

## Done when

- [ ] Seven days recorded, none with an open fault that loses data or blocks
      work.
- [ ] The tuned gesture values committed with the owner's sign-off.
