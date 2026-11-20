# Ticket 213 — The Better OS taskbar at the bottom of the session

**Spec:** better-os-session.md S4 (taskbar row); ticket 142 item 3
**Blocked by:** 200, 203, 210, 152, and the owner's decision S3

## Why

Ticket 142 item 3 and `design/top-bar-spec.md` section 10 (decisions D3 (c)
and D4): a Better OS taskbar with the Better OS button beside Show
Applications, no clock, system menu, or tray while the top bar runs. On GNOME
it needs a second row in ADR 0036's program table and waits with ticket 151;
in the session it is a layer surface with no shell code.

## What to build

- **Design first, with whoever holds `design/top-bar-spec.md`**: section 10's
  "Better OS taskbar" column as a full section with every state and copy and
  mockups light and dark, in ticket 148's style, reviewed with the owner
  before building.
- **Crate `taskbar`** (GPUI-free model: pinned applications, running windows
  grouped by application, order, attention, what shrinks at each width) and a
  layer-shell view, bottom edge, exclusive zone, on each output the top bar's
  "every display" setting covers.
- **Left end**: the Better OS button opening Better Launcher anchored to it
  (ticket 210), and Show Applications opening what decision S3 chose; D4's
  switch to shape (b) when ticket 94 makes the launcher a complete start menu.
- **Windows**: `zcosmic_toplevel_info_v1` and `zcosmic_toplevel_manager_v1`
  (activate, minimize on a second click, close and the window actions in a
  menu), pinned applications launched through the shared platform launch path,
  pin and unpin, drag to reorder, kept in `$XDG_STATE_HOME/better-os/taskbar/`.
- **Package and component**: a user unit `WantedBy=better-os-session.target`,
  part of the session component or its own (choose with the design), with its
  `paths`.

## Done when

- [ ] The design section and mockups in the repository and the owner's
      answer recorded.
- [ ] Unit tests for the model in both languages and each width step.
- [ ] In the VM session with the top bar: pinned, running, minimized, and
      several windows of one application; a maximized window between the two
      strips; screenshots in `/home/tim/Developer/better-os-evidence/213/`
      compared with the mockups.
- [ ] fmt, clippy on stable, the crates' tests, and `verify-deb.sh` pass.
