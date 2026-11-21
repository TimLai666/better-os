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

The dconf profile question this ticket raised is settled by ticket 203: the
session follows the machine's `user` profile. Neither Ubuntu 24.04 nor Zorin
OS 18.1 ships one (dconf's default `user-db:user` then), so the shipped
`betteros` profile stands; where an administrator's `/etc/dconf/profile/user`
adds `system-db` lines, the launcher writes a profile with them under the
session's runtime directory and `DCONF_PROFILE` names its path, so an
administrator's lockdown (saving disabled) reaches Better Screenshot in the
session as in GNOME.

## Done when

- [x] The owner's answer is recorded in this ticket.
- [ ] Every feature in "What to build" works in the nested `cosmic-comp`
      (`packaging/tests/cosmic-nested.sh`) with fixture windows, in zh-TW and
      English, light and dark, at 100 % and 150 %.
- [x] In ticket 201's VM (`packaging/tests/session-vm/`), Print Screen in the
      session saves an image of the screen where the person expects it, and an
      application asking the Screenshot portal interactively gets a dialog
      and an image; screenshots in `/home/tim/Developer/better-os-evidence/253/`.
- [x] Tests for the program's rules (save path, file name, refusal when the
      portal says no).
- [ ] S typed as the interface opens picks the screen. In the Better OS
      session (ticket 203's VM) it did not, while P did take effect (notes,
      "Checked in the Better OS session").
- [ ] Enter captures after a mode button was clicked. In the session it
      pressed the clicked Screen button again, which held the keyboard focus
      (`crates/screenshot-gui/src/view.rs`: the keys go to the root's
      `on_key_down`, and a focused button takes Enter first).

**Blocked:** several outputs (the VM's virtual GPU shows its second head
disconnected with QEMU's headless display; ticket 201's VM plan for several
outputs), the recording's indicator in a real top bar (tickets 152, 156),
and the non-interactive portal call through COSMIC's own Access backend.
The two key defects above are open and not blocked: their cause is in Better
Screenshot's own focus and key handling, not in the session's data (no
session shortcut uses S or Enter), and they are fixed here. Everything else
ran in the real session (notes, "Checked in the Better OS session").

## Notes (2026-11-22)

### What is built

- `engineering/specs/screenshot.md`: every feature of GNOME Shell 46's
  interface as a promise (O, C, P, K, S, R, A rows) with where GNOME does
  it, what `cosmic-comp` and `xdg-desktop-portal-cosmic` serve, and how it is
  built.
- `crates/screenshot-core` (no GPUI, no Wayland, 57 tests): GNOME's area
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
`better-os-evidence/253/<lang>-<theme>-<scale>/` (beside the repository):
screenshots 01–13, the saved picture and recording, the clipboard read back
with `wl-paste`, the portal client's answers, the notifications, the recent
files, and the state file.

| GNOME 46 feature (spec row) | Built | Verified how |
| --- | --- | --- |
| Print opens the interface over every output, frozen screen (O1, O2) | yes | nested, every run (01) |
| Opening during a recording stops it instead (O3, K3, R6) | yes | `--record` stopped a running recording in every run (08, 09); core test |
| Escape and the close button close without a picture; close on the desktop's side (O4) | yes | Escape (11), portal answer 1; close side from GTK's `button-layout`, left side not seen |
| Fade in 200 ms (O5) | yes | GPUI's animation, easing out as Clutter's `EASE_OUT_QUAD` |
| Fade out 200 ms, the frozen screen included, no input meanwhile (O5) | yes | captured 60 ms after Escape half faded (`fade-and-lockdown/02-fading-out.png`: the shade at (100, 100) read 19, then 34, then 39 closed) |
| Print during the fade out brings it back (O5) | yes | core test (`Shown::Closing` → `Reopen`); nested: Escape then Print, the interface was open after (first fix round's run), but whether the second Print landed inside the 200 ms was not measured |
| The session's lock (O9) | yes | core test of the decision; logind's `Lock` and `LockedHint` are read from the system bus and were not exercised (no test may lock the owner's session; ticket 201's VM) |
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
| Area remembered and reset (C9) | yes | the portal's interface reopened on the last area; core tests; kept in the login's runtime directory since the review |
| Area across outputs (C10) | yes | core and controller tests with two outputs; one nested output only |
| Screen: choose by click and arrows (C11) | yes | 05; arrows by core test |
| Window: windows laid out, focused one chosen, check mark (C12) | yes | 06; at 150 % the six runs laid the fixture out at its buffer's size divided by the scale (a GPUI window drawing at 1), fixed to the compositor's logical width and seen again (`after-fixes/02-window-150.png`); arrows by core test |
| Window unavailable without windows and in recording mode (C13) | yes | 07 (greyed); core tests |
| Window picture is the window's own buffer (C14) | yes | `Alt+Print` saved 720 × 420 of the fixture |
| Show Pointer (P1) | yes | the switch and copies work (04); `cosmic-comp`'s source paints the pointer into a `paint_cursors` copy (`render.rs` 356–359, 404, 422; windows 617–649), but the nested compositor's copies came out equal: ticket 201's VM |
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
| Saving disabled (S6, A5) | yes | the lock from a dconf-compiled database read through a sandbox profile (`fade-and-lockdown/`): Enter put a PNG on the clipboard (`wl-paste`) and saved nothing, the notification had no actions, the recording was refused with "Screencast failed to start", the portal's instant picture answered 2, PickColor still answered 0; `defaults-platform` `dconf_stack` tests (10) on fixture databases |
| Notification with image, open, Show in Files (S7, R7) | yes | the stand-in service got title, body, image path, actions, and no `transient` hint; its "Show in Files" press reached `FileManager1.ShowItems` with the file; its body press opened the picture in a fixture viewer named the default for `image/png` in the sandbox's `mimeapps.list` (`open-from-notification/viewer.log`) |
| Shift+Print and Alt+Print without the interface (S8) | yes | `--screen` 1280 × 800, `--window` 720 × 420 |
| Recording of an area or a screen, 30 fps, pointer, no sound (R1) | yes | WebM VP8 at the area's size × scale, video only (`recording-discoverer.txt`), through `ext-image-copy-capture` (the spec says why not the ScreenCast portal); GNOME 46's recorder has no audio element in any pipeline either (`screencastService.js` 33–79, 480–486) |
| Interface closes at once when recording starts (R2) | yes | 08 |
| Shading outside the recorded area (R3) | yes | 08 |
| Time indicator with stop (R4) | yes | the bar (08), stopped by pressing it once by hand; the tray item's `Title` read "Recording the screen, 0:03" and its `Activate` stopped the recording (`after-fixes/tray-item.txt`); no watcher runs in the session yet to show it |
| WebM under Videos/Screencasts, GNOME's names (R5) | yes | every run |
| Failure notices (R8) | yes | core tests; disk full not provoked |
| Portal interactive: interface, URI, cancel (A1) | yes | real frontend: response 0 with the URI (12), Escape → 1 |
| Portal non-interactive: flash, URI, a file only (A2) | yes | real frontend after the permission store said yes: response 0 with `file://…/Screenshot.png` in the home folder (the sandbox had no Pictures), clipboard empty before and after, no notification, no recent list (first fix round's run); "Deny" in the Access dialog → response 2, no file |
| PickColor (A3) | yes | real frontend: response 0 with `(ddd)` (13) |
| Request.Close, the caller leaving, one request at a time, only the frontend served (A4) | yes | Close closed the interface (first fix round's run); a PickColor stayed pending through a Shift+Print and was answered by the click (first fix round's run); the caller leaving and a caller other than the frontend by private-bus tests; busy and binding by core tests |

### What is left, and why

- **Ticket 201's VM (Blocked by 203):** Print Screen in the real session, the
  pointer in a picture (the nested compositor draws no pointer of its own),
  the tray item in a real top bar, several outputs, and the portal with
  COSMIC's own Access backend.
- **An open session decision (the owner's or ticket 203's):** whether the
  session's shipped dconf profile `/usr/share/dconf/profile/betteros`
  (better-os-session.md, environment table: `user-db:betteros` then
  `user-db:user`) also lists the administrator's system databases. As
  decided, a lockdown an administrator puts in `/etc/dconf/db/local`
  reaches the session only through an `/etc/dconf/profile/betteros` of
  their own, which dconf reads before the shipped one; GNOME sessions read
  it when `/etc/dconf/profile/user` lists it. Better Screenshot reads
  whatever profile is in force, so either answer needs no change here.
- **Untested:** the lockdown through a real `system-db` line (the sandbox
  cannot write `/etc/dconf/db/`; the nested run used a `file-db` line,
  which dconf opens and locks the same way, and the reader's tests cover
  `system-db` paths).
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

### Added on 2026-11-22 (coordinator's follow-up)

- The fade out (O5) and saving disabled (S6, A5), above; the lockdown reader
  is `crates/defaults-platform/src/dconf_stack.rs` with `gvdb.rs` reading a
  database's `.locks` table, on fixture databases compiled by dconf 0.40's
  own `dconf compile` (`tests/fixtures/dconf/system-local-locked`,
  `system-site`, `user-lockdown-off`).
- `better-os-session.md`'s Screenshot and media keys rows now name Better
  Screenshot.
- The copies of the StatusNotifierItem code and of the FileManager1 call:
  tickets 300 and 301.

### The independent review of 2026-11-22

`better-os-evidence/253/review-2026-11-22.md`; every finding was checked
against the code and fixed. Each fix has a test that fails without it unless
said otherwise.

- **P1 portal answered by another capture:** `screenshot_core::portal::
  PortalSlot` binds the pending request to the capture it started
  (`Origin`); a person's capture never answers it (`slot_tests`), and the
  interface, pictures, and `pick` answer only their own origin.
- **P2 a stop while the recording starts:** `recording::Phase` remembers
  the stop and the start's completion stops the recorder, waits, and deletes
  the file; `Recording` stops and joins its thread on drop
  (`dropping_a_recording_stops_its_thread_and_waits_for_it`); the first-frame
  timeout joins the thread before the file is deleted.
- **P3 unbounded `appsrc`:** two frames, oldest dropped, and a two-frame
  queue (`a_slow_encoder_makes_the_source_drop_frames_rather_than_queue_them`:
  39 frames waiting before, at most 2 after). 2880 × 1800 nested recording,
  34 s of a moving picture: 372 MB peak in its scope, with or without the
  bound on this machine (`memory/`).
- **P4 opening from the notification:** `screenshot_gui::open` launches the
  `mimeapps.list` default through `app-chooser-core` and
  `app-catalog-platform`'s launcher, then the portal's `OpenFile` with a
  descriptor; Show in Files falls back to `OpenDirectory` with one. Seen in
  the nested run; the portal fallbacks were not run.
- **P2-level: the portal's instant picture** now writes only
  `Screenshot.png` (spec A2; `the_portals_instant_picture_goes_to_pictures_or_home_as_screenshot`).
- **The lockdown read** fails closed: `platform::saving_from` (test
  `only_dconfs_null_configuration_reads_as_allowed`), `lockdown::Saving`;
  `dconf_stack` reads a COSMIC or Better OS process without `DCONF_PROFILE`
  as dconf does (`resolve_profile_as_dconf`), and a damaged `.locks` is noted
  with the values still read (`gvdb` and `dconf_stack` tests).
- **P3-level:** a caller leaving the bus closes its request and only the
  frontend is served (private-bus tests in `service.rs`); Show Pointer and
  the area are kept per login (spec C9, P1); the lock (O9); notifications
  not transient, the copied-only one with `image-data`; the notification
  listener ends at its deadline without any signal (private-bus test);
  outputs without `xdg_output` or a picture are left out with the reason
  (`assemble_outputs` and `OutputState` tests); the current mode only;
  the vendored protocol notices are in `docs/third-party-licenses.md` from
  `packaging/generate-third-party-notices.sh`; Better Screenshot reads the
  XDG data directories through `files_platform::xdg` and keeps no state
  path of its own, and ticket 302 folds the other copies; the recorded
  path is no longer printed; Print during the fade reopens.
- **Transforms:** the rotations were applied the wrong way round; now the
  inverse, tested against the protocol's definition and seen at `_90`,
  `Flipped90`, and `Flipped180` in the nested compositor; `_270` could not
  be checked there (spec, "The compositor").
- **Who may call the backend:** recorded in the spec's portal section; only
  the frontend's name owner is answered.

Retaken after the fixes: `zh_TW-dark-1`, `en_US-light-1.5`, and the window
mode at 150 % in both languages and both themes (`zh_TW-dark-1.5`,
`zh_TW-light-1.5`, `en_US-dark-1.5`, `en_US-light-1.5`, screenshot 06).

### The second review of 2026-11-22

`better-os-evidence/253/review-2-2026-11-22.md`: the twenty first-round
items confirmed fixed; three new ones, fixed.

- **A portal call while locked, or a lock while its interface opens, was
  never answered** (the slot stayed taken; every later call was "already
  showing"; the process could not leave or restart on update). Every way an
  interface or a request ends now goes through
  `screenshot_core::portal::Requests`: a call while locked is answered
  "cancelled" and takes no slot; the interface owes its request an answer
  from the moment it starts opening until its picture is handed over;
  `close_instantly` (Print over a portal's fade, an output change, a lock)
  ends an interface that is still opening as well, and the copy that comes
  back opens nothing. `request_tests` drive done, failed, cancel, Close
  (before and after the copy is back), a lock while opening and at the
  copy's return, an output change, and the fade-out reopen, and assert the
  slot is empty and exactly one answer went out.
- **A close during the fade after Capture said "cancelled" for the
  person's picture:** Capture hands the picture over (`handed_over`), and
  later closes no longer answer for it; the store sends the picture (same
  test, "done, then anything that closes the interface during the fade").
- **A damaged name of any `H` table failed the whole dconf database:** now a
  lock problem, values still read
  (`a_table_whose_name_cannot_be_read_is_a_lock_problem_not_a_failure`,
  which fails on the parser before this change).
- The account name is out of the text evidence (paths now `~/…`).
- Seen in the nested compositor while the owner's own session was locked
  (logind `LockedHint` true; the watcher reads the session the process
  runs in, read only): the portal's instant picture, an interactive call,
  and PickColor each heard 1 at once and none heard 2, Print opened
  nothing, and Shift+Print still saved and notified
  (`review-checks-locked/`). That run showed a call arriving before the
  first `LockedHint` message was served (response 0,
  `review-checks-locked-before-startup-read/`), so the lock is now read
  before the first bus message is handled. The first fix round's
  `review-checks` folder (unlocked) was overwritten by that run; its
  results are the ones recorded in the table above. The unlocked paths
  after this round are covered by `request_tests` and were not run again in
  the nested compositor, because the owner's session stayed locked.

### Checked in the Better OS session (2026-11-22, ticket 203's VM)

Ubuntu 24.04 VM (`packaging/tests/session-vm/`), the closure from System76's
source, the `better-os-session` package built from ticket 203's branch with
everything listed in "What ticket 203 adds" above, one 1920×1080 output, a
real GDM login. Screenshots and files in
`/home/tim/Developer/better-os-evidence/253/vm-session-203/`.

| Check | Result |
| --- | --- |
| Print opens the interface | yes, over the frozen screen (21) |
| Screen picture with the pointer (P on) | saved 1920×1080, the pointer drawn where it was (24, 25) |
| Window picture with the pointer, from the interface | saved 656×547, the I-beam drawn inside the window (27, 28) |
| Alt+Print | saved the focused window, 656×547, without the pointer (26) |
| Shift+Print | saved the whole screen, 1920×1080 (`shift-print.png`) |
| An application's interactive portal call | `org.freedesktop.portal.Screenshot.Screenshot` with `interactive` from a client that stays on the bus: the interface opened (30), and after Capture the Request answered `response=0` with the file's URI (`python-client.txt`; the client is `/home/tim/Developer/better-os-evidence/203/scripts/portal-screenshot.py`) |
| Recording | Ctrl+Shift+Alt+R opened the interface in recording mode (31), Screen and Capture started it (32), Ctrl+Shift+Alt+R again stopped it: `Videos/Screencasts/Screencast from … .webm`, WebM VP8 1920×1080, 7.5 s (`gst-discoverer-1.0`) |
| Notifications | every capture logged "no notification shown: org.freedesktop.Notifications was not provided": the session has no notification service until ticket 215 |

Two defects, open in "Done when": S typed while the interface opened did
not switch to Screen (P did take effect), and Enter after clicking the
Screen button pressed that button again rather than capturing (it held the
keyboard focus); the checks above went on with the capture button. Not run
here: several outputs (QEMU's headless virtual GPU reports its second head
disconnected), the recording's indicator in a real top bar, and the
non-interactive portal call with COSMIC's Access dialog.
