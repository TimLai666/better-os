# Spec: Better Screenshot, the screenshot and screen recording interface of the Better OS session

**Tickets:** 253 (this program), 203 (the session package it ships in)
**Owner's decision (2026-11-21, ticket 253):** Better OS writes its own
screenshot program in Rust, and it does everything GNOME 46's screenshot
interface does. `cosmic-screenshot` is not added, so C2's closure stays at 14
packages (ADR 0038).

## Goal

In the Better OS session (better-os-session.md, S4's "Screenshot" row)
Print Screen does nothing, because COSMIC's `system_actions` names
`cosmic-screenshot`, which the closure does not install. Better Screenshot
(`better-screenshot`, app id `io.betteros.Screenshot`) is the interface a
person gets instead: the one GNOME Shell 46 draws over the screen, with the
same three ways of choosing, the same keys, the same files in the same
folders, and the same answers to an application that asks the Screenshot
portal.

## What GNOME 46 does, and what Better Screenshot promises

Read from GNOME Shell 46.0's source (`js/ui/screenshot.js`, 3123 lines;
`js/dbusServices/screencast/screencastService.js`; `js/ui/status/remoteAccess.js`;
`data/org.gnome.shell.gschema.xml.in`; `po/zh_TW.po`; fetched with
`gh api repos/GNOME/gnome-shell/contents/<path>?ref=46.0` on 2026-11-21).
Line numbers are `screenshot.js`'s unless another file is named. Each row is a
promise; ticket 253's notes say how each was checked.

### Opening and the frozen screen

| # | Promise | Where GNOME does it |
| --- | --- | --- |
| O1 | Print Screen opens the interface over every output. | gschema `show-screenshot-ui` = `Print`; `showScreenshotUI` 2410 |
| O2 | The interface shows the screen as it was when it opened (a frozen picture of every output), and what is captured is that picture, so the interface itself, menus, and anything that changes after the key never get into the picture. | `open` 1612–1634 (`screenshot_stage_to_content`), `_saveScreenshot` 1893 |
| O3 | Opening while a recording runs does not open the interface (Better Screenshot stops the recording instead, R6). | `open` 1569 |
| O4 | Escape, the close button, or the interface losing its grab closes it without capturing. The close button sits at the panel's top corner on the side the desktop's window buttons put close. | `_closeButton` 1197–1218, `_refreshButtonLayout` 1486, `grab` 1652 |
| O5 | The interface fades in and out over 200 ms. | `open` 1665, `close` 1710 |
| O6 | An output added or removed while the interface is open closes it. | `monitors-changed` 1394 |
| O7 | The panel (type buttons, picture/recording switch, capture button, pointer switch) is on the primary output's bottom centre, 4 em above the edge. Better Screenshot puts it on the output the pointer is on, because COSMIC has no primary output. | `_primaryMonitorBin` 1183, `_screenshot.scss` `.screenshot-ui-panel` |
| O8 | Every button has a tooltip: "Area Selection", "Screen Selection", "Window Selection", "Take Screenshot", "Record Screen", "Capture", "Show Pointer". | 1263–1377 |

### Choosing what is captured

| # | Promise | Where GNOME does it |
| --- | --- | --- |
| C1 | Three types: Selection (an area), Screen, Window; exactly one is chosen; Selection is chosen when the interface opens unless the last choice was kept. | `_selectionButton` 1254, `_onSelectionButtonToggled` 1718, `_onScreenButtonToggled` 1749, `_onWindowButtonToggled` 1779 |
| C2 | Area: outside the area is shaded, the area has four round corner handles. | `UIAreaIndicator` 124, `UIAreaSelector` 252–332 |
| C3 | Area: a press outside the area, with the secondary button, or with Ctrl held starts a new area from that point; dragging draws it. | `_onPress` 440–452 |
| C4 | Area: a press inside moves it; on an edge (within 10 px × scale outside it) resizes that edge; on a corner (within half a handle of it, or the 10 px corner square) resizes both edges. The pointer shape says which (move, the four sides, the four corners, crosshair outside). | `_computeCursorType` 334–378, `_onPress` 453–499 |
| C5 | Area: dragging an edge past the opposite edge turns the area over and the handle becomes the opposite one. | `_onMotion` 617–651 |
| C6 | Area: moving keeps the size when it reaches a screen edge, and the pointer then slides relative to the area ("rubber band"); resizing stops at the screen edges. | `_onMotion` 547–615 |
| C7 | Area: a click without a drag makes a 40 px × scale square around the point, kept inside the screen. | `stopDrag` 392–421 |
| C8 | Area: while dragging, the panel and close button fade to 100/255. | `drag-started`/`drag-ended` 1220–1243 |
| C9 | Area: the area is kept for the next time the interface opens; it is reset to the middle quarter of the primary output (from 3/8 to 5/8 of each side) when it no longer fits the outputs. Better Screenshot keeps it across runs in its state file, since it is not a resident shell. | `reset` 284–312 |
| C10 | Area: an area may span outputs (the stage is one surface in GNOME). | `UIAreaSelector` binds to the stage |
| C11 | Screen: each output is a button; the primary is chosen when the interface opens (Better Screenshot: the one with the pointer); clicking one chooses it; arrow keys move between them. | `_rebuildMonitorBins` 1529–1562, key handler 2177–2200 |
| C12 | Window: the windows of the current workspace on each output, without override-redirect windows, are laid out over that output as in the overview, each a picture of the window as it was; the focused window is chosen at open; clicking one chooses it; arrow keys move between them; the chosen one has a border and a check mark. | `UIWindowSelector.capture` 1009–1041, `UIWindowSelectorLayout` 710, `UIWindowSelectorWindowContent` 775–929 |
| C13 | Window: the Window button is unavailable when there are no windows, and in recording mode. | `_syncWindowButtonSensitivity` 1476, `_onCastButtonToggled` 1839–1843 |
| C14 | Window: the picture is the window's own buffer at its own scale, not cut from the screen, so what covers it is not in it. | `_saveScreenshot` 1917–1948 |

### Options

| # | Promise | Where GNOME does it |
| --- | --- | --- |
| P1 | "Show Pointer" puts the pointer into the picture (area, screen, or window) or the recording; it is off when the interface opens; while on, the frozen pointer is drawn over the frozen screen. | `_showPointerButton` 1366–1388, `_saveScreenshot` 1900–1931, `_startScreencast` 1960 |
| P2 | The picture/recording switch: two buttons (camera and video camera), exactly one on; recording mode hides the frozen picture so the live screen shows, and gives the capture button its red recording look. | `_shotButton`/`_castButton` 1309–1341, `_onShotButtonToggled` 1809, `_onCastButtonToggled` 1825 |
| P3 | The recording button is shown only when recording is supported, and is unavailable when the interface was opened for a picture only (the portal). | `_syncCastButton` 1467 |
| P4 | Closing the interface switches back to picture mode. | `_finishClosing` 1682 |

### Keys

| # | Promise | Where GNOME does it |
| --- | --- | --- |
| K1 | Print opens the interface; Shift+Print takes every output at once; Alt+Print takes the focused window at once; Ctrl+Shift+Alt+R opens the interface in recording mode. | gschema 285–300, `addKeybinding` 1406–1456 |
| K2 | In the interface: Enter, keypad Enter, ISO Enter, Space, and Ctrl+C capture; S chooses Selection; C chooses Screen; W chooses Window (when it is available); P switches the pointer; V switches picture and recording (when recording is available); arrow keys move between screens or windows; Escape leaves. | `vfunc_key_press_event` 2140–2204 |
| K3 | Ctrl+Shift+Alt+R while recording stops it (the owner's minimum; GNOME 46 ignores the key then, `open` 1569). | ticket 253 |

### The picture

| # | Promise | Where GNOME does it |
| --- | --- | --- |
| S1 | Capture closes the interface and plays the "screen-capture" sound of the sound theme. | `_onCaptureButtonClicked` 1879, `captureScreenshot` 2392 |
| S2 | The picture is PNG, at the output's own pixels (the highest scale of the outputs an area covers). | `captureScreenshot` 2386, `composite_to_stream` |
| S3 | It goes to the clipboard as `image/png` first, so a failed save still leaves it there. | `_storeScreenshot` 2215–2217 |
| S4 | It is saved in `<Pictures>/<Screenshots>/<Screenshot from %Y-%m-%d %H-%M-%S>.png`, where Pictures is the person's `XDG_PICTURES_DIR` from `xdg-user-dirs` (the home folder when it has none) and the folder and file words are GNOME's translations (English "Screenshots", "Screenshot from %s"; zh-TW 「螢幕快照」, 「螢幕快照 %s」), so a person coming from GNOME finds them in the same folder; the folder is created when missing; an existing name gets `-1`, `-2`, … and is never overwritten. | `_storeScreenshot` 2272–2306, `po/zh_TW.po` |
| S5 | The saved file is added to the recently used files (`$XDG_DATA_HOME/recently-used.xbel`). | `saveRecentFile` 2245–2265 |
| S6 | When saving to disk is locked down (`org.gnome.desktop.lockdown disable-save-to-disk`), nothing is saved and the clipboard and notification still happen, without "Show in Files". | 2267–2272, 2343 |
| S7 | A notification "Screenshot captured" / "You can paste the image from the clipboard." from "Screenshot", with the picture as its image; clicking it opens the file; its "Show in Files" action shows the file in the file manager. | 2326–2368 |
| S8 | Shift+Print and Alt+Print go through the same S1–S7 without any interface. | 1422–1456 |

### The recording

| # | Promise | Where GNOME does it |
| --- | --- | --- |
| R1 | A recording covers the chosen area or screen (never a window), records the live screen, with or without the pointer as P1 says, at 30 frames per second. | `_startScreencast` 1955–2025, service `DEFAULT_FRAMERATE` |
| R2 | The interface closes at once (no fade) when recording starts, so the fade is not recorded. | 1975 |
| R3 | While it runs, outside the recorded area is shaded on every output, and nothing of that shading is in the recording. | `_screencastAreaIndicator` 1141–1156 |
| R4 | While it runs, an indicator shows the time as `m:ss`, counting each second from `0:00`, with a stop icon; pressing it stops the recording. GNOME draws it in the top bar; until Better OS's top bar exists, Better Screenshot draws it as its own small bar at the top of the output it records, and also offers a tray item that stops it. | `remoteAccess.js` `ScreenRecordingIndicator` 68–135 |
| R5 | It is saved as `<Videos>/<Screencasts>/<Screencast from %Y-%m-%d %H-%M-%S>.webm` (VP8 in WebM), Videos from `xdg-user-dirs` (home when none), folder created when missing; zh-TW's folder is 「螢幕錄影」 and the file word stays English, because GNOME 46's zh_TW translation of "Screencast from %d %t" is the stale one with `.webm` and so is not used (`po/zh_TW.po` line 1797). | `_startScreencast` 1994–2005, service `_generateFilePath` 575–620, `_getAbsolutePath` 564 |
| R6 | It stops from the indicator, the tray item, Ctrl+Shift+Alt+R, or Print. | ticket 253, `stopScreencast` 2027 |
| R7 | After it stops: a notification "Screencast recorded" with "Click here to view the video." that opens the video, and "Show in Files"; the file is added to the recently used files. | `stopScreencast` 2049, `_showNotification` 2084–2126, service `_addRecentItem` |
| R8 | A failure says which: "Screencast failed to start", "Screencast ended: Out of disk space", "Screencast ended unexpectedly". | `_screencastFailed` 2052–2082 |

### The Screenshot portal

| # | Promise | Where GNOME does it |
| --- | --- | --- |
| A1 | An application's interactive `org.freedesktop.portal.Screenshot.Screenshot` opens the same interface in picture-only mode (no recording button); Capture answers response 0 with the saved file's URI; leaving answers response 1 (cancelled). | `InteractiveScreenshotAsync` 2673–2699, `UIMode.SCREENSHOT_ONLY` |
| A2 | A non-interactive call (the portal frontend has already asked the person through the Access dialog) takes every output at once, with a white flash over each that fades over 500 ms and the shutter sound, and answers its URI. | `ScreenshotAsync` 2650, `Flashspot` 3096–3123 |
| A3 | `PickColor` shows the frozen screen with the pointer hidden and a colour-pick preview following it; a click answers the pixel's colour as `(ddd)` in 0–1; Escape answers cancelled. | `PickColorAsync` 2745, `PickPixel` 3004–3094 |
| A4 | The portal's `Request.Close` on a pending call closes the interface and answers cancelled; a second call while one is pending is refused as busy. | `_createScreenshot` 2449 |
| A5 | Saving locked down (S6) refuses a call that needs the disk. | `_createScreenshot` 2454 |

### Not offered, and why

- GNOME Shell's private `org.gnome.Shell.Screenshot` and
  `org.gnome.Shell.Screencast` D-Bus interfaces (`ScreenshotArea`,
  `SelectArea`, `FlashArea`, …). GNOME limits them to its own settings
  daemon, its portal backend, and GNOME Screenshot (`DBusSenderChecker`
  2431); in the session the Screenshot portal and Better Screenshot's own
  `org.betteros.Screenshot1` are the interface.
- GNOME's pipeline blocklist and H.264 first (service `PIPELINES`): it tries
  `openh264enc` into MP4 before VP8 into WebM. `openh264enc` is in
  `gstreamer1.0-plugins-bad`, which an Ubuntu desktop does not install by
  default and which needs `libopenh264` from outside the archive; VP8
  (`vp8enc`, `libgstvpx.so`) and WebM (`webmmux`, `libgstmatroska.so`) are in
  `gstreamer1.0-plugins-good` on 22.04 (1.20.3) and 24.04 (1.24.2), which
  every Ubuntu desktop has (packages.ubuntu.com file lists, read
  2026-11-21). So Better Screenshot records WebM VP8, with GNOME's own VP8
  settings, and has no list to fall back through.
- Touch: GNOME's area selector takes touch sequences (`vfunc_touch_event`
  687). GPUI at the pinned revision delivers no touch events, so touch on a
  touch screen arrives as the pointer emulation the compositor gives, if any.

## The compositor: what `cosmic-comp` serves

Facts from the nested `cosmic-comp` 0.1~1795167359~24.04~6968c5d
(`packaging/tests/cosmic-nested.sh`, `better-wl-probe globals`, 2026-11-21),
the same build ticket 201 ran on both bases. Ticket 253's notes have how each
capture was run.

| Need | Protocol | Served |
| --- | --- | --- |
| A picture of an output | `ext_image_copy_capture_manager_v1` 1 with `ext_output_image_capture_source_manager_v1` 1 | yes |
| A picture of one window | `ext_foreign_toplevel_image_capture_source_manager_v1` 1 | yes |
| The window list | `ext_foreign_toplevel_list_v1` 1 (handles, title, app id) and `zcosmic_toplevel_info_v1` 3 (outputs, workspaces, states including activated, geometry) | yes |
| The current workspace | `ext_workspace_manager_v1` 1 | yes |
| The pointer in a picture | `ext_image_copy_capture_manager_v1.create_session` with `paint_cursors` | see the notes |
| The clipboard without focus | `ext_data_control_manager_v1` 1 (and `zwlr_data_control_manager_v1` 2) | yes |
| Surfaces over every output | `zwlr_layer_shell_v1` 5 | yes |
| A workspace picture | `zcosmic_workspace_image_capture_source_manager_v1` 1 | yes, unused |
| `wlr-screencopy` | `zwlr_screencopy_manager_v1` | **no** |

`xdg-desktop-portal-cosmic` 1.11.0~1795035856~24.04~7b0864c (the closure's
build; source at that commit, `src/screencast.rs`, `src/screenshot.rs`):

- Its ScreenCast `SelectSources` shows COSMIC's own source picker
  (`screencast_dialog::show_screencast_prompt`, `screencast.rs` 235) unless
  the call carries restore data from an earlier persisted choice. A recording
  through the ScreenCast portal would therefore show COSMIC's picker after
  Better Screenshot's own interface, every time. Better Screenshot records
  through `ext-image-copy-capture` itself, the protocol that portal uses
  underneath, and encodes with GStreamer; no second dialog, no helper of
  another desktop.
- Its Screenshot backend has an interactive editor of its own
  (`screenshot.rs` 486–540, shown when `interactive` is true), and
  `cosmic-screenshot` is only a command that asks the portal for it. The
  owner's decision stands: the session's portal configuration names Better
  Screenshot for `org.freedesktop.impl.portal.Screenshot`, so applications
  get the same interface as Print Screen.

## How it is built

- `screenshot-core` (no GPUI, no Wayland): the area rules of C2–C10 (press,
  motion, release, pointer shape, click enlargement, clamping, rubber band,
  turning over, the remembered area and its reset), the interface's state
  (type, picture or recording, pointer, which screen and window, what each
  key does, what is available), the window layout, composing a picture from
  frozen outputs at their scales with the pointer, the file names and
  folders by `xdg-user-dirs`, the recent-files entry, the recording's time
  and states, the notification words, and the portal's answers.
- `screenshot-capture`: the `Capture` trait (outputs, windows, pictures, the
  pointer, the clipboard, recording frames) with a fake for tests, and the
  Wayland implementation on its own connection (GPUI's clipboard writes text
  only on Wayland, `gpui_linux` `wayland/client.rs` `write_to_clipboard`,
  and GPUI binds none of the capture protocols). The recording encoder
  (GStreamer `appsrc ! videoconvert ! vp8enc ! webmmux ! filesink`).
- `screenshot-gui`, binary `better-screenshot`: the interface on
  `better_ui::layer` surfaces (one per output, `Layer::Overlay`, keyboard
  exclusive, filled), the recording's bar and shading, the D-Bus names, the
  portal backend, notifications, the tray item, and AGENTS.md's window rules
  (command line through `better_ui::command_line`, `UpdateRestart`, `app_id`).

### One process, its names, and its life

`better-screenshot` is one process at a time. The first owns
`org.betteros.Screenshot` (interface `org.betteros.Screenshot1`) and
`org.freedesktop.impl.portal.desktop.betteros.Screenshot` (the portal
backend at `/org/freedesktop/portal/desktop`); a later start with an
argument hands the argument to it and exits:

| Command | Key | Does |
| --- | --- | --- |
| `better-screenshot` | Print | opens the interface; stops a running recording |
| `better-screenshot --screen` | Shift+Print | every output at once |
| `better-screenshot --window` | Alt+Print | the focused window at once |
| `better-screenshot --record` | Ctrl+Shift+Alt+R | the interface in recording mode; stops a running recording |
| `better-screenshot --portal` | (D-Bus activation) | serves the portal and waits |

The process stays while the interface is open, while a recording runs, while
it holds the clipboard (an `ext_data_control` source lives only as long as
its client; it leaves when another program takes the clipboard), while a
notification it raised is open, and while a portal call is pending; then it
exits.

### What ticket 203's package adds

- `system_actions`' `Screenshot` names `better-screenshot`; the session's
  keys for Shift+Print, Alt+Print, and Ctrl+Shift+Alt+R name the commands
  above.
- `betteros-portals.conf` gets
  `org.freedesktop.impl.portal.Screenshot=betteros-screenshot;` (the name is
  the `.portal` file's base name, xdg-desktop-portal 1.18.4
  `portal-impl.c` 181–182).
- The package ships `/usr/bin/better-screenshot`,
  `/usr/share/xdg-desktop-portal/portals/betteros-screenshot.portal`, the
  D-Bus activation file, and `io.betteros.Screenshot.desktop`, as
  `packaging/screenshot/` holds them.
