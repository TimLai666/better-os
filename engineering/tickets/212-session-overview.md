# Ticket 212 — The overview shows every window and workspace in the session

**Spec:** better-os-session.md S4, S6 (overview, application windows)
**Blocked by:** 200, 203

## Why

`cosmic-workspaces` is not installed; the session needs its own overview, and
the Mac-style preset's four fingers up (overview) and down (the current
application's windows) need a surface to open. ADR 0038 sizes it large:
window thumbnails through `ext-image-copy-capture`, workspaces through
`ext_workspace_v1`.

## What to build

- **Crate `overview`**: GPUI-free layout (windows of each workspace arranged
  without overlap, the current application's windows only in the
  application-windows mode, a workspace strip), and a layer-shell view on the
  overlay layer per output.
- **Thumbnails** through `ext-image-copy-capture-v1` and
  `ext-image-capture-source-v1` for each toplevel, into shared-memory buffers
  drawn as images; first measure the cost per window and per frame with 20
  windows at scale 2 and record it in Notes, and refresh only visible
  thumbnails at a rate the measurement allows.
- **Actions**: click activates, drag a window to another workspace
  (`zcosmic_toplevel_manager_v1` move to workspace), close from the
  thumbnail, new workspace, keyboard navigation, type-to-search handing the
  text to Better Launcher.
- **Opening**: Super+W (or the binding ticket 203 sets) and the taskbar;
  the gesture transition (kind overview and application windows) as
  `TransitionUpdate` progress drives the layout between the windows' real
  places and the overview places (accepted in a test here; the gesture itself
  is ticket 224).
- Accessible names and roles; copy in both languages.

## Done when

- [ ] Unit tests for the layout (1, 2, 10, 30 windows; mixed sizes; several
      outputs) and the interpolation at progress 0, 0.5, 1, and below 0.
- [ ] The capture cost measured and recorded; idle cost with the overview
      closed is zero captures.
- [ ] In the VM session, both modes with windows on two workspaces;
      screenshots light and dark in `/home/tim/Developer/better-os-evidence/212/`.
- [ ] fmt, clippy on stable, the crates' tests, and `verify-deb.sh` pass.
