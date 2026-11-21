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

