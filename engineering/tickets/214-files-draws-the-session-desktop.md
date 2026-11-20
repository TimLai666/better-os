# Ticket 214 — Better Files draws the desktop in the session

**Spec:** better-os-session.md S4, S5 (Better Files row; C6)
**Blocked by:** 200, 203

## Why

C6: Better Files draws the desktop's icons. In the session COSMIC Files'
desktop process is not started, so without this the desktop is empty and
`org.freedesktop.FileManager1` has no owner. Ticket 144's stacks build on it.
`~/Desktop` is never moved (ADR 0038).

## What to build

- **The desktop view**: a GPUI layer surface on the background layer per
  output (`better_ui::layer`), showing `~/Desktop` through `files-core` and
  Better Files' own views, icon sizes, thumbnails, and kind rules; free
  placement with positions kept per output in
  `$XDG_STATE_HOME/better-os/files/desktop.json`; select, rubber-band, open,
  rename, drag and drop to and from windows, the context menu with Better
  Files' operations through `files-operations` jobs; mounted drives and the
  Trash as options; keyboard navigation and screen reader names.
- **A resident process** for it, a user unit `WantedBy=better-os-session.target`
  in the `better-files` package, which also owns
  `org.freedesktop.FileManager1` in the session (ticket 129's service).
- **Several monitors**: each output its own icons (not repeated), a removed
  output's icons moved to the primary keeping their order.
- Under GNOME nothing of this starts; Zorin's extension keeps drawing there
  (C6: the GNOME row waits with ticket 151).

## Done when

- [ ] Unit tests for placement, a new file landing in the first free cell,
      positions surviving a rename, several outputs, an output removed.
- [ ] In the VM session: `~/Desktop`'s files shown, opened, dragged into a
      Better Files window and back, renamed, trashed; FileManager1's
      `ShowItems` from Firefox's downloads opens Better Files; screenshots
      light and dark in `/home/tim/Developer/better-os-evidence/214/`.
- [ ] fmt, clippy on stable, the crates' tests, and `verify-deb.sh` pass.
