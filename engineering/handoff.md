# Handoff

Unfinished work that has no ticket. Delete a section when its work is done, and the file when it is empty.

## Observations owed at a real desktop
- **Asked:** each of these was left by a released ticket as something only a person at a real Zorin or Ubuntu desktop can confirm; none was checked on screen.
- **Done and checked:** each behavior is covered by tests or by the host probe's output, not by watching it happen.
- **Tried and failed:** nothing; these were never attempted on a desktop.
- **Next:** on a real desktop, check each one and remove it from this list:
  - GNOME draws the six component icons in the applications grid after an install (ticket 43).
  - Pressing Escape closes Better Launcher (tickets 43, 61).
  - Better Manager's sidebar footer reads `Zorin OS 18 · Ubuntu 24.04 base` on a Zorin 18 machine, looked at rather than asserted from the host probe (ticket 45).
  - The Better Defaults row for a component that is already the XDG default (ticket 48).
  - A real machine installs Better Awake through Better Manager and keeps it (ticket 47).
  - The new icons in every window on the owner's own desktop, in both appearances: every screen of ticket 147 came from a headless GNOME Shell (ticket 147).
  - An object picture drawn large (Better Launcher's 64px preview, a 48px device banner) on a display scaled above 2x, where GPUI's fixed 2x raster may still look soft (ticket 147).
  - Better Git with a repository open (its rows, chips, and Push and Pull buttons) and Better Manager's pages other than Home, with the new icons (ticket 147).
- **Sources:** tickets 43, 45, 47, 48, 61 and 147.

## Work in flight when the session of 2026-11-22 stopped
- **Asked:** finish every open ticket; the owner paused the session to update the application, which ends its background workers.
- **Done and checked:** pull request 30 (the login and lock design, ADR 0044 accepted, L1 B, L2 a, L8 a) is merged. Ticket 253's decision (Better OS's own screenshot program, as rich as GNOME's) and ADR 0045 (GPUI Kit on GPUI Fast, measured) are on main.
- **Next, in this order:**
  - **Pull request 31 (ticket 101, branch `ticket-101`):** its independent review finished with no P0 or P1, four P2, five P3 and one item to investigate; the full text is in `/home/tim/Developer/better-os-evidence/101/review-2026-11-22.md`. Fix all of them in the worktree `.claude/worktrees/agent-a0b0c26fa1796ed24` before merging: validate the module file before unloading it; give the notification child no inherited journal stream and accept only the daemon's two marker lines from its stream; never power off while docked, while another session of any uid exists, or while the daemon's busy marker exists; mark a reload as done only when the charger is back; record the cooldown on failed runs; a timeout on the notification; count the hub work as hung only inside a hung-task report; use Better Awake's locale rule. Then read the checks for the new head and merge on GitHub.
  - **The login and lock design's review:** an independent review found one P0 and nine P1 in the merged design (the supervisor's `PartOf=` stops itself on every retry; the 60-second arithmetic; the unlock socket's directory mode; switching off ends the running session; and more). Its full text is in `/home/tim/Developer/better-os-evidence/259/design-review-2026-11-22.md`. A worker had started revising `design/login-screen-spec.md`, `engineering/specs/login-screen-security.md`, ADR 0044 and tickets 259 to 281 in the worktree `.claude/worktrees/agent-ae6d97c181f9293c1` (one file changed, nothing committed). Finish the revision for every finding, push it as `ticket-259-design-review`, and open a pull request. No login or lock code (tickets 260 to 281) starts before that is merged.
  - **Ticket 253 (the screenshot program):** the worker finished its build and pushed nothing itself; its two commits are now the draft pull request from branch `ticket-253` (worktree `.claude/worktrees/agent-a4618b636f029ffba`). Still to do there: the fade-out when the interface closes; the lockdown setting (needs a reader of the system dconf databases and their locks); an independent review and its fixes; the Screenshot row of `engineering/specs/better-os-session.md`; then merge. The StatusNotifierItem code now exists three times and the "show this file" call twice: move each into one shared place (its own ticket). The VM check waits for ticket 203, whose additions the ticket's notes list.
  - **Ticket 257:** the upgrade to GPUI Kit's current release on GPUI Fast is not started; ADR 0045 lists the edits and the two things the owner accepts with it.
- **Sources:** tickets 101, 253, 257, 259; ADR 0044, 0045, 0046 (on `ticket-101`).
