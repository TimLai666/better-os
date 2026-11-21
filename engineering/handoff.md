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


## Release 0.2.17 before the desktop session
- **Asked:** owner, 2026-11-22: ticket 310 goes ahead of the queue, and a release is published before the Better OS session is built.
- **Done and checked:** nothing of the release itself yet.
- **Next:** when pull requests 31 (ticket 101) and 34 (ticket 272) and ticket 310 are merged and main's CI is green, cut 0.2.17 by `engineering/specs/release-packaging.md`: bump the version (and `SELF_UPGRADING_SINCE`/`HANDOFF_SINCE` only as AGENTS.md says), build all packages for 22.04 and 24.04 on amd64 and arm64 in CI, publish, download the public assets again, write their checksums into the manifests, and correct the comment above each block. Not in this release: the screenshot program (no package until ticket 203), the tray's power-mode item (waits for the icon design), anything of the session or the login path. After it is installed on the owner's machine, the confirmations that waited for a release are theirs to give: tickets 131, 134, 136, 170, 129, and 101's measurement.
- **Sources:** tickets 101, 272, 310; `engineering/specs/release-packaging.md`.
