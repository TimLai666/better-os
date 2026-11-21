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

## Done when

- [x] The owner's answer is recorded in this ticket.
- [ ] Every feature in "What to build" works in the nested `cosmic-comp`
      (`packaging/tests/cosmic-nested.sh`) with fixture windows, in zh-TW and
      English, light and dark, at 100 % and 150 %.
- [ ] In ticket 201's VM (`packaging/tests/session-vm/`), Print Screen in the
      session saves an image of the screen where the person expects it, and an
      application asking the Screenshot portal interactively gets a dialog
      and an image; screenshots in `/home/tim/Developer/better-os-evidence/253/`.
- [x] Tests for the program's rules (save path, file name, refusal when the
      portal says no).

**Blocked:** the VM check needs ticket 203's session package, which puts
`better-screenshot` on Print Screen and in the session's portal
configuration; until it exists nothing in a real session starts the program.

## Notes (2026-11-22)

### What is built

- `engineering/specs/screenshot.md`: every feature of GNOME Shell 46's
  interface as a promise (O, C, P, K, S, R, A rows) with where GNOME does
  it, what `cosmic-comp` and `xdg-desktop-portal-cosmic` serve, and how it is
  built.
- `crates/screenshot-core` (no GPUI, no Wayland, 55 tests): GNOME's area
  selector rule for rule (`UIAreaSelector`), the interface's state and keys,
  the window layout, composing a picture from frozen outputs at their scales
  and transforms, GNOME's file names and folders in both languages, the
  recently used files, the recording's time, the portal's answers, the
  process's life.
- `crates/screenshot-capture` (4 tests, and the `capture-probe` and
  `pointer-drag` examples): `ext-image-copy-capture-v1` on a Wayland
  connection of its own for every output and window (each twice, without
  and with `paint_cursors`), the window list through
  `ext-foreign-toplevel-list-v1` and `zcosmic_toplevel_info_v1` (XML vendored
  from `cosmic-protocols` 815b2c5, X11-style licence), the current workspace
  through `ext-workspace-v1`, the clipboard through `ext-data-control-v1`,
  and recording into WebM VP8 with GStreamer (`appsrc ! videoconvert !
  vp8enc ! webmmux`, GNOME's VP8 settings); a fake behind the `Capture` trait.
- `crates/screenshot-gui`, `better-screenshot` (13 tests): the interface on
  `better_ui::layer` surfaces over every output (overlay layer, exclusive
  keyboard), the kit's buttons and tooltips, the recording's shading and bar,
  one process owning `org.betteros.Screenshot` (`org.betteros.Screenshot1`:
  `Request`, `StopRecording`, `Recording`, `RecordingSeconds`) and
  `org.freedesktop.impl.portal.desktop.betteros.Screenshot` (the Screenshot
  portal backend, version 2, with `Request.Close`), notifications with "Show
  in Files" through `org.freedesktop.FileManager1`, a StatusNotifierItem
  while recording, `better_ui::command_line`, `UpdateRestart` on the panel's
  surface and an update watch that ends an idle process, `app_id`
  `io.betteros.Screenshot`.
- `packaging/screenshot/`: `betteros-screenshot.portal`, the backend's D-Bus
  activation file, `io.betteros.Screenshot.desktop`. `packaging/build-deb.sh`
  is not changed: its structure is one package per component, and the
  program's package is ticket 203's, which does not exist yet.
- `Cargo.lock` gained only the three crates; the license inventory is
  regenerated.

### How it was checked

Nested `cosmic-comp` 0.1~1795167359 (`packaging/tests/cosmic-nested.sh`,
state in `~/.cache/better-os/cosmic-nested-253`), a GPUI fixture window
(`better-layer-check --toplevel`), the sandbox's own `HOME`, the real
`xdg-desktop-portal` 1.18.4 frontend and permission store on the sandbox's
bus started by D-Bus activation from the sandbox's service directory with
only `betteros-screenshot.portal` and GTK's Access backend configured, and a
stand-in notification service and file manager (test scripts, not in the
repository). Drags are a `zwlr_virtual_pointer_v1` on the outer sway
(`pointer-drag` example). Six runs of the same sequence, English and zh-TW,
light and dark, 100 % and 150 %, each in
`/home/tim/Developer/better-os-evidence/253/<lang>-<theme>-<scale>/`:
screenshots 01–13, the saved picture and recording, the clipboard read back
with `wl-paste`, the portal client's answers, the notifications, the recent
files, and the state file.

| GNOME 46 feature (spec row) | Built | Verified how |
| --- | --- | --- |
| Print opens the interface over every output, frozen screen (O1, O2) | yes | nested, every run (01) |
| Opening during a recording stops it instead (O3, K3, R6) | yes | `--record` stopped a running recording in every run (08, 09); core test |
| Escape and the close button close without a picture; close on the desktop's side (O4) | yes | Escape (11), portal answer 1; close side from GTK's `button-layout`, left side not seen |
| Fade in 200 ms (O5) | yes | rendered with GPUI's animation; not captured mid-fade |
| Fade out (O5) | **no** | see the spec |
| An output change closes the interface (O6) | yes | not exercised: the nested compositor has one output |
| Panel on the pointer's output, tooltips (O7, O8) | yes | nested (01); cursor session `enter` read |
| Three types, one chosen, kept (C1) | yes | S, C, W keys and buttons (05, 06); state file |
| Area: shade, border, four handles (C2) | yes | 01 |
| Area: draw, secondary button, Ctrl (C3) | yes | drawn by drag (01); secondary and Ctrl by core tests |
| Area: move, resize by edge and corner, pointer shapes (C4) | yes | drags (02, 03), shapes visible; core tests |
| Area: turning over (C5) | yes | 03 (south-west handle dragged past the north-east); core test |
| Area: rubber band at the edge (C6) | yes | core test |
| Area: click makes a 40 px square (C7) | yes | core test |
| Area: panel fades while dragging (C8) | yes | code; not captured mid-drag |
| Area remembered and reset (C9) | yes | the portal's interface reopened on the last area; core tests |
| Area across outputs (C10) | yes | core and controller tests with two outputs; one nested output only |
| Screen: choose by click and arrows (C11) | yes | 05; arrows by core test |
| Window: windows laid out, focused one chosen, check mark (C12) | yes | 06; at 150 % the six runs laid the fixture out at its buffer's size divided by the scale (a GPUI window drawing at 1), fixed to the compositor's logical width and seen again (`after-fixes/02-window-150.png`); arrows by core test |
| Window unavailable without windows and in recording mode (C13) | yes | 07 (greyed); core tests |
| Window picture is the window's own buffer (C14) | yes | `Alt+Print` saved 720 × 420 of the fixture |
| Show Pointer (P1) | yes | the switch and copies work (04); the nested compositor draws no pointer, so both copies are equal: ticket 201's VM |
| Picture/recording switch, red capture button (P2) | yes | 07 |
| Recording button unavailable for the portal (P3) | yes | 12 |
| Back to picture mode on close (P4) | yes | each reopening |
| Keys: Print, Shift+Print, Alt+Print, Ctrl+Shift+Alt+R (K1) | yes | the commands ticket 203 binds: `better-screenshot`, `--screen`, `--window`, `--record`, handed to the running process |
| Keys inside: Enter, Space, Ctrl+C, Escape, S, C, W, P, V, arrows (K2) | yes | Enter, Escape, S, C, W, P, V on the virtual keyboard; all by core test |
| Shutter sound (S1) | yes | the freedesktop theme's `screen-capture` through GStreamer; not heard (no audio in the sandbox) |
| PNG at the outputs' own pixels (S2) | yes | 150 % runs saved at 1.5 × the logical size |
| Clipboard first (S3) | yes | `wl-paste --type image/png` gave the saved file's bytes |
| Saved under Pictures/Screenshots with GNOME's names, both languages, suffixes (S4) | yes | `Screenshot from …` and `螢幕快照/螢幕快照 …`; core tests |
| Recently used files (S5) | yes | `recently-used.xbel` in each run |
| Lockdown (S6, A5) | **no** | see the spec |
| Notification with image, open, Show in Files (S7, R7) | yes | the stand-in service got title, body, image path, actions; its "Show in Files" press reached `FileManager1.ShowItems` with the file |
| Shift+Print and Alt+Print without the interface (S8) | yes | `--screen` 1280 × 800, `--window` 720 × 420 |
| Recording of an area or a screen, 30 fps, pointer (R1) | yes | WebM VP8 at the area's size × scale (`recording-discoverer.txt`) |
| Interface closes at once when recording starts (R2) | yes | 08 |
| Shading outside the recorded area (R3) | yes | 08 |
| Time indicator with stop (R4) | yes | the bar (08), stopped by pressing it once by hand; the tray item's `Title` read "Recording the screen, 0:03" and its `Activate` stopped the recording (`after-fixes/tray-item.txt`); no watcher runs in the session yet to show it |
| WebM under Videos/Screencasts, GNOME's names (R5) | yes | every run |
| Failure notices (R8) | yes | core tests; disk full not provoked |
| Portal interactive: interface, URI, cancel (A1) | yes | real frontend: response 0 with the URI (12), Escape → 1 |
| Portal non-interactive: flash, URI (A2) | yes | real frontend after the permission store said yes: response 0, the flash captured; "Deny" in the Access dialog → response 2, no file |
| PickColor (A3) | yes | real frontend: response 0 with `(ddd)` (13) |
| Request.Close; one request at a time (A4) | yes | Close closed the interface; busy refusal by core test |

### What is left, and why

- **Ticket 201's VM (Blocked by 203):** Print Screen in the real session, the
  pointer in a picture (the nested compositor draws no pointer of its own),
  the tray item in a real top bar, several outputs, and the portal with
  COSMIC's own Access backend.
- **Fade out and lockdown** (spec, "What is not built yet"): the lockdown
  needs a reader of the dconf profile's system databases and their locks,
  which Better OS does not have.
- **What ticket 203 adds** to its package: `/usr/bin/better-screenshot`
  (`cargo build -p screenshot-gui`); the three files of
  `packaging/screenshot/` under `/usr/share/xdg-desktop-portal/portals/`,
  `/usr/share/dbus-1/services/`, and `/usr/share/applications/`;
  `Depends: gstreamer1.0-plugins-base, gstreamer1.0-plugins-good` (appsrc,
  videoconvert, playbin; vp8enc, webmmux); in the session's
  `system_actions` default `Screenshot: "better-screenshot"`; custom
  bindings Shift+Print → `better-screenshot --screen`, Alt+Print →
  `better-screenshot --window`, Ctrl+Shift+Alt+R → `better-screenshot
  --record` (COSMIC's `system_actions` has one `Screenshot` action,
  `cosmic-settings-daemon` `data/system_actions.ron`); and in
  `betteros-portals.conf` the line
  `org.freedesktop.impl.portal.Screenshot=betteros-screenshot;` beside
  `default=cosmic;gtk;` (the frontend offers the Screenshot portal only
  with an Access backend). Its test that every action names a shipped
  program then covers `better-screenshot`.
- **better-os-session.md** still says "nobody yet" in the process table's
  Screenshot row; that file was outside this ticket's files.
