# Ticket 253 — Print Screen and the Screenshot portal's dialog work in the Better OS session

**Spec:** better-os-session.md S4 (the "Screenshot" and "Media keys" rows of the process table)
**Blocked by:** 203

## Why

Ticket 201 found that COSMIC's default `system_actions` binds the screenshot
key to `cosmic-screenshot`, which is not one of the 14 packages of the pinned
closure, so Print Screen does nothing in the session. Under GNOME the shell
takes screenshots. `xdg-desktop-portal-cosmic` serves the Screenshot portal
(its non-interactive path), but a person pressing Print Screen gets nothing.

## Owner's decision (2026-11-21)

Better OS writes its own screenshot program in Rust, and it does everything
GNOME 46's screenshot interface does. Adding `cosmic-screenshot` is not taken,
so C2's closure stays at 14 packages.

## What to build

A Better OS screenshot program (GPUI, opened through `better_ui::layer` over
every output), with every feature of GNOME Shell 46's screenshot interface
(`js/ui/screenshot.js`, read at that version before building):

- Three ways to choose what is captured: an area (drawn by dragging, then moved
  and resized by its edges and corners, with its size remembered for next
  time), a whole screen (one of several outputs picked by pointing), and one
  window (picked from the open windows).
- A switch for whether the pointer is in the picture.
- A still picture or a recording of the screen: the same interface switches
  between the two, a recording covers an area or a screen, shows that it is
  running and how long, and is stopped from the top bar or the tray and by
  pressing the key again.
- A picture is copied to the clipboard and saved as
  `~/Pictures/Screenshots/Screenshot from <date> <time>.png` (the person's
  translated Pictures folder from `xdg-user-dirs`), a recording under
  `~/Videos/Screencasts`; a notification shows it and opens it or its folder.
- The keys GNOME gives: Print Screen opens the interface, Shift+Print takes the
  whole screen at once, Alt+Print the focused window at once,
  Ctrl+Shift+Alt+R starts and stops a recording; in the interface Enter,
  Space, and Ctrl+C capture, Escape leaves, S, C, and W pick the screen,
  area (selection), and window, V switches picture and recording, P the
  pointer.
- The Screenshot portal's interactive call shows the same interface, so an
  application asking for a screenshot gets it and the image.
- Any feature of GNOME 46's interface found in its source and not listed here
  is built too; one that the compositor cannot serve is named in the notes
  with the protocol that is missing, and the ticket stays open for it.

The pixels come from the compositor's capture protocols
(`ext-image-copy-capture-v1` with `ext-image-capture-source-v1`, or what
`cosmic-comp` at the pinned version serves; a recording through the
ScreenCast portal and PipeWire), never from a helper of another desktop.
The session's `system_actions` default (ticket 203's package data) names the
program; ticket 203's test that every action names a shipped program covers
it. The program ships in the session's package and follows AGENTS.md's rules
for a window (restart on update, command line, `app_id`).

What ticket 203 left for this ticket (2026-11-22), each place marked "Ticket
253 adds": in `packaging/build-deb.sh`'s `better-os-session` block the
binary, the three `packaging/screenshot/` files, and the two GStreamer
`Depends`; in `packaging/session/betteros-portals.conf` the Screenshot line;
in `packaging/session/cosmic/com.system76.CosmicSettings.Shortcuts/v1/`
`system_actions` `Screenshot: "better-screenshot"` and in `defaults` Print →
`System(Screenshot)` and the three `Spawn(...)` bindings; and in
`crates/session-launcher/tests/shortcuts.rs` the `Screenshot` line of
`WAITING` comes out. The session's `defaults` is the whole binding table (a
configuration's defaults come from one directory, better-os-session.md's
`XDG_DATA_DIRS` row), so the bindings go there and not in `custom`.

## Done when

- [x] The owner's answer is recorded in this ticket.
- [ ] Every feature in "What to build" works in the nested `cosmic-comp`
      (`packaging/tests/cosmic-nested.sh`) with fixture windows, in zh-TW and
      English, light and dark, at 100 % and 150 %.
- [ ] In ticket 201's VM (`packaging/tests/session-vm/`), Print Screen in the
      session saves an image of the screen where the person expects it, and an
      application asking the Screenshot portal interactively gets a dialog
      and an image; screenshots in `/home/tim/Developer/better-os-evidence/253/`.
- [ ] Tests for the program's rules (save path, file name, refusal when the
      portal says no).
