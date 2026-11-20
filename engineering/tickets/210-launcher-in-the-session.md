# Ticket 210 — Super+Space opens Better Launcher as a panel in the session, and Super+A opens every application

**Spec:** better-os-session.md S4, S5 (Better Launcher row)
**Blocked by:** 200, 203, 94 (the resident, wide launcher), and the owner's decision S3

## Why

Ticket 94 makes Better Launcher a resident, summoned panel and binds
Super+Space on GNOME, where proving how it is drawn needs shell code. In the
session it is a layer surface and the binding is the session's own; COSMIC's
own launcher and app library are not installed, so Super, Super+Space, and
Super+A must reach Better Launcher or nothing.

## What to build

- **The panel as a layer surface** through `better_ui::layer`: overlay
  layer, keyboard interactivity exclusive while shown, no exclusive zone,
  centred a little above the middle of the output the pointer is on (ticket
  94's shape), hidden on Escape, focus loss, or launch; anchored to the
  taskbar's Better OS button when opened from it (ticket 213).
- **Key bindings** in the session's defaults (ticket 203): Super alone and
  Super+Space run `better-launcher --toggle`; Super+A runs `better-launcher
  --all-apps`; COSMIC's `InputSourceSwitch` on Super+Space is unbound and
  Fcitx 5 takes Alt+Shift (ticket 208).
- **All apps** as decision S3 chooses (recommended: a full-screen grid on the
  overlay layer).
- **File search** as ticket 94 builds it, with Tracker started by the session
  target (ticket 203); where ticket 201 found Tracker absent, the Files
  sections say Tracker is not running, never empty results.
- The gesture's transition (ticket 224) plugs into this surface; nothing for
  it here beyond accepting `TransitionUpdate` for kind launcher in a test.

## Done when

- [ ] Unit tests for placement per output and scale and for the key-binding
      table.
- [ ] In the VM session: Super+Space shows the panel at once, types,
      launches, hides; Super+A shows All apps; the panel appears on the output
      with the pointer; screenshots light and dark in
      `/home/tim/Developer/better-os-evidence/210/`.
- [ ] fmt, clippy on stable, and the crates' tests pass.
