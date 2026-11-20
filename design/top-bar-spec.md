# Better OS top bar

A bar across the top of the screen: the Better OS menu and the focused
application's name on the left, the tray, input method, network, sound,
battery, clock, and notifications on the right. It does not carry
applications' menus: every window keeps its own menu row, and the bar offers
what a window does not have (owner, 2026-11-21, decision D5). This file says
what the bar contains, how each part behaves in every state, the exact copy in
both languages, how it is built on GNOME 46, and how it sits with the taskbar
at the bottom. Its shell decision is
[ADR 0036](/engineering/decisions/0036-top-bar-dock-surface.md) (accepted);
the work is cut into tickets 149, 150 to 152, and 156 to 160. The look follows
`DESIGN.md`, and every control named here is a `better_ui::fluent` control.
The mockups in `design/top-bar/` draw the bar in both appearances, in zh-TW, on
a 1440 × 900 logical screen (the owner's display at scale 2) with Zorin's
taskbar as decision D3 (a) leaves it: `rest-betteros` (a Better OS window
focused) and `rest-other` (another application), which have the same shape,
`betteros-menu`, `app-actions` (the application name's menu open),
`tray-overflow`, `clock-flyout`, and `rest-960` (a 960-wide screen), each as
`-light.html` and `-dark.html`. The English copy is in the tables below.

Where this file and a ticket differ, the ticket decides behaviour and this file
decides layout and copy.

## 0. The owner's decisions

All seven were answered by the owner on 2026-11-21. D5 and D6 were answered
differently from what the first version of this design recommended, and the
sections below follow the answers.

| # | Question | Answer (2026-11-21) | Refused |
| --- | --- | --- | --- |
| D1 | Which shape the bar is | **(A)** A Better OS window drawn by Rust and GPUI, which GNOME Shell makes a dock and places at the top (section 9.3). | (B) GNOME's own top panel moved back to the top, with menus drawn in JavaScript (section 9.5). |
| D2 | ADR 0036, which widens ADR 0018's bounds for the panel relay | **Accepted.** | Refusing it, which leaves no top bar on GNOME. |
| D3 | Zorin's bottom taskbar while the bar is on | **(a) now:** it stays, minus its clock and system menu. **(c) once ticket 142's taskbar exists:** that taskbar replaces it. Section 10. | (b) Zorin's taskbar turned into a centred, hiding dock. |
| D4 | Ticket 142's taskbar left end | **(a)** the Better OS button beside Show Applications, **until ticket 94 makes Better Launcher a complete start menu, then (b)** the Better OS button replaces Zorin's start button. | Going to (b) before Better Launcher is a complete start menu. |
| D5 | Applications' menus | **The bar carries no application menus at all.** The owner: the bar is not there to replace a window's menus but to offer other things. A window keeps its own menu row, always. The left side is the Better OS menu and the focused application's name, which opens the actions every window has (section 4). Nothing registers with or serves `com.canonical.AppMenu.Registrar`, `better-ui` exports no `com.canonical.dbusmenu` menus, no window hides its menu row, and the bar never shows File, Edit, or View for any application, Better OS's own included. | Global menus in the bar, with a Better OS window's own row hidden (the first recommendation) or kept as well. Section 9.7 records what that would have taken. |
| D6 | Old X11 (XEmbed) tray icons | **Shown in the bar too**, through a small Rust bridge of Better OS's own that takes the XEmbed tray selection on XWayland and offers each icon to the bar as a StatusNotifierItem, as KDE's `xembedsniproxy` does (sections 6.1 and 9.6). Zorin's AppIndicator extension is still off while the bar is on, which is what frees the selection. | Not showing XEmbed icons while the bar is on (the first recommendation); keeping Zorin's AppIndicator extension on beside the bar. |
| D7 | Notifications in the bar | **(a)** The clock opens a Better OS flyout that lists GNOME's notifications, relayed from the shell to the bar service only (section 6.7). | (b) The clock opens GNOME's own calendar and list, which on Zorin opens from the bottom taskbar. |

## Sources this design was checked against

Read on the owner's machine on 2026-11-21, without writing anything:

- Zorin OS 18.1, GNOME Shell 46.0, Mutter `46.2-1ubuntu0.24.04.18+zorin1`,
  Wayland, one display `eDP-1` (Samsung `SDC 0x419d`) at 2880 × 1800, scale
  2.0, so 1440 × 900 logical pixels. `org.gnome.mutter experimental-features`
  is `[]`.
- The Wayland globals the compositor advertises, listed by a client that bound
  the registry and nothing else (there is no `wayland-info` on the machine):
  `gtk_shell1` v5, `wl_compositor` v6, `wl_data_device_manager` v3,
  `wl_output` v4, `wl_seat` v8, `wl_shm`, `wl_subcompositor`,
  `wp_fractional_scale_manager_v1`, `wp_presentation`,
  `wp_single_pixel_buffer_manager_v1`, `wp_viewporter`, `xdg_activation_v1`,
  `xdg_wm_base` v6, `zwp_idle_inhibit_manager_v1`,
  `zwp_keyboard_shortcuts_inhibit_manager_v1`, `zwp_linux_dmabuf_v1` v5,
  `zwp_pointer_constraints_v1`, `zwp_pointer_gestures_v1` v3,
  `zwp_primary_selection_device_manager_v1`, `zwp_relative_pointer_manager_v1`,
  `zwp_tablet_manager_v2`, `zwp_text_input_manager_v3`, `zxdg_exporter_v1`/`v2`,
  `zxdg_importer_v1`/`v2`, `zxdg_output_manager_v1` v3. No
  `zwlr_layer_shell_v1`, no `ext_layer_shell`, no `org_kde_kwin_appmenu`. The
  interface names compiled into `libmutter-14.so.0.0.0` (`strings`) contain no
  layer-shell interface either.
- `libmutter-14.so.0.0.0`'s exported symbols and the `Meta-14.typelib` strings:
  `meta_wayland_client_make_dock`, `make_desktop`, `hide_from_window_list`,
  `owns_window`, `spawnv` are callable from GJS; `meta_wayland_client_new_indirect`
  and `meta_wayland_client_setup_fd` exist in the library but are not in the
  typelib. `meta_window_get_gtk_menubar_object_path`,
  `get_gtk_unique_bus_name`, `get_gtk_application_id`, `get_pid`,
  `meta_window_x11_get_xwindow`, `meta_display_get_monitor_in_fullscreen`,
  `meta_window_move_frame`, `make_fullscreen`, `minimize`, `delete` exist.
- Zorin's taskbar `zorin-taskbar@zorinos.com` (`/usr/share/gnome-shell/extensions/`):
  `panelManager.js:693-698` takes `Main.panel` out of GNOME's top `panelBox`
  and puts it in the bottom panel unless `stockgs-keep-top-panel` is true
  (`panelManager.js:67`, read `false`); `panel.js:120-170` gives every panel in
  "keep the top panel" mode its own date menu and system menu. The person's
  settings: `panel-position 'BOTTOM'`, `panel-sizes {"SDC-0x00000000":45}`,
  `multi-monitors false`, and `panel-element-positions` with `leftBox`,
  `showAppsButton`, `taskbar`, `centerBox`, `activitiesButton`, `rightBox`,
  `systemMenu`, `dateMenu`, `desktopButton`, all visible.
- Zorin's AppIndicator extension `zorin-appindicator@zorinos.com` owns
  `org.kde.StatusNotifierWatcher` (`busctl --user list`: owner `gnome-shell`),
  ships `statusNotifierWatcher.js` and `trayIconsManager.js`, and has
  `legacy-tray-enabled true`, `tray-pos 'right'`; five items were registered.
  Its XEmbed tray is GNOME Shell's own `Shell.TrayManager`, which the extension
  creates (`trayIconsManager.js:67`); GNOME Shell holds no tray selection of
  its own while the extension is off.
- XWayland: package `xwayland 2:23.2.6-1ubuntu0.8`. It starts on demand: the
  socket `/tmp/.X11-unix/X0` existed (created at login) while no `Xwayland`
  process ran (`pgrep -x Xwayland` found none). When it starts, Mutter runs
  `/etc/xdg/Xwayland-session.d/` (`00-at-spi`, `00-xrdb`, `10-ibus-x11` on the
  machine) and the user units of `gnome-session-x11-services.target`
  (`org.gnome.SettingsDaemon.XSettings.service`, which is `PartOf` that target,
  in `/usr/lib/systemd/user/`).
- `org.freedesktop.Notifications` and `org.gtk.Notifications` are owned by
  GNOME Shell. `pipewire-pulse`, `ibus-daemon --panel disable` (GNOME Shell is
  the ibus panel), and `gsd-power` run; NetworkManager and UPower are on the
  system bus. Input sources `[('ibus', 'chewing')]`, `clock-format '12h'`,
  `clock-show-weekday true`, `clock-show-date true`, `enable-hot-corners false`,
  `LANG=zh_TW.UTF-8`.
- No GNOME key binding uses `<Control>F2` (the wm, shell, mutter, and
  media-keys schemas were listed; `<Alt>F2` is the run dialog);
  `toggle-message-tray` is `['<Super>v', '<Super>m']`, `focus-active-notification`
  `['<Super>n']`, and there are no custom key bindings.
- GPUI at the locked revision `ae394f3` (`Cargo.lock`), in the cargo git
  checkout: `gpui_linux/src/linux/platform.rs:616-628` stores the menus
  `set_menus` is given and returns them from `get_menus`; nothing exports them
  (`set_dock_menu` is `// todo(linux)`). No file under `crates/gpui*` mentions
  `dbusmenu`, `com.canonical`, or `gtk_shell`. GPUI has
  `WindowKind::LayerShell` (bound from `zwlr_layer_shell_v1`,
  `wayland/client.rs:259`, refused with `LayerShellNotSupportedError` where the
  compositor lacks it) and `WindowKind::AnchoredPopup` (`xdg_popup`,
  `platform.rs:2008-2014`). It connects with `Connection::connect_to_env()`
  (`wayland/client.rs:639`), which takes `WAYLAND_SOCKET` first
  (`wayland-client-0.31.15/src/conn.rs:46-52`).
- `Cargo.lock`: `x11rb 0.13.2` and `x11rb-protocol 0.13.2`
  (`MIT OR Apache-2.0`, `docs/third-party-licenses.md:1100-1101`), pulled in by
  `gpui_linux`, which enables the `xkb`, `randr`, `xinput`, `cursor`,
  `resource_manager`, `sync`, and `dri3` features
  (`gpui_linux/Cargo.toml:120-129`). x11rb 0.13.2's own `Cargo.toml` has
  `composite`, `damage`, `xfixes`, `shape`, and `xtest` features that only turn
  on modules of `x11rb-protocol` (lines 77-137). `xcb 1.7.0` (`MIT`) is also in
  the lock and links libxcb.
- Better OS's own code: Better Git draws File, View, Repository, and Branch in
  its titlebar (`crates/git-gui/src/render.rs:583-960`); `awake-tray` and
  `monitor-tray` each serve their own `com.canonical.dbusmenu`
  (`crates/awake-tray/src/dbusmenu.rs`, `crates/monitor-tray/src/lib.rs`).

Read from upstream sources on 2026-11-21:

- Mutter `gnome-46`, `src/wayland/meta-wayland-client.c`
  (<https://raw.githubusercontent.com/GNOME/mutter/gnome-46/src/wayland/meta-wayland-client.c>):
  `make_dock` calls `meta_window_set_type (window, META_WINDOW_DOCK)` for a
  normal window the client owns; `hide_from_window_list` sets
  `skip_from_window_list`; `setup_fd` works only with `new_indirect`.
- Mutter `gnome-46`, `src/core/window.c`
  (<https://raw.githubusercontent.com/GNOME/mutter/gnome-46/src/core/window.c>):
  a dock is not focused on a click ("Don't focus panels--they must explicitly
  request focus", in `meta_window_handle_ungrabbed_event`), is not focused when
  it maps, gets `skip_taskbar` and `skip_pager` and `always_sticky`, and its
  layer is `META_LAYER_DOCK`, or `META_LAYER_BOTTOM` while its monitor has a
  fullscreen window. The base `meta_window_real_update_struts` returns
  `FALSE`; only the X11 window class reads struts, so a Wayland window
  reserves no space by itself.
- Mutter issue 973, "implement layer_shell protocol"
  (<https://gitlab.gnome.org/GNOME/mutter/-/issues/973>) and issue 1053, where
  the maintainers wrote that Mutter does not want to support arbitrary third
  party panels (<https://gitlab.gnome.org/GNOME/mutter/issues/1053>).
- KDE's `xembedsniproxy`, plasma-workspace `master`, directory
  `xembed-sni-proxy/` (<https://invent.kde.org/plasma/plasma-workspace/-/tree/master/xembed-sni-proxy>),
  files `fdoselectionmanager.cpp`, `sniproxy.cpp`, `xcbutils.h`,
  `xtestsender.cpp`, and `CMakeLists.txt`, read on 2026-11-21 at the commit
  that last changed `sniproxy.cpp`, `6e3f75f1` (2026-05-01, "xembed-sni-proxy:
  fix scroll event race condition on Wayland"). Licence
  `LGPL-2.1-or-later`; the bridge in section 9.6 follows its method and copies
  none of its code. What each part does is cited by line in section 9.6.
- Section 9.7's sources on global menus: GTK `gtkapplication-wayland.c`, KDE
  bug 424485, Chromium's `dbus_appmenu_registrar.cc` and
  `wayland_toplevel_window.cc`, Mozilla bug 1883184, Qt's `qwaylandappmenu_p.h`
  and KDE review D3085, with their URLs there.
- libcosmic's panel applet chapter
  (<https://pop-os.github.io/libcosmic-book/panel-applets.html>) and the Arch
  wiki's COSMIC page (<https://wiki.archlinux.org/title/Cosmic>).

## 1. The bar

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│ ◆  Better Files                                  ▴ ◈ ◈ ◈  酷  ᯤ  🔊  🔋 87%  11月21日 週六 下午3:42  🔔 │ 32
└─────────────────────────────────────────────────────────────────────────────────────────┘
 ▲  ▲                                             ▲          ▲   ▲   ▲    ▲         ▲               ▲
 │  app name (its window actions)                 overflow   tray    IME net  sound battery  clock      notifications
 Better OS menu
```

- **Height** 32 logical pixels at every scale (a standard control height in
  `DESIGN.md`); 64 physical pixels at scale 2. Full width of the monitor it is
  on.
- **Fill**: the window background (`#f3f3f3` light, `#202020` dark) with a 1px
  bottom edge in the card stroke. No shadow, no radius: it meets the screen's
  edges. The bar follows the person's appearance like every Better OS window
  (ADR 0019).
- **Items**: each item is a button 32px tall, 8px side padding, with the
  subtle fill on hover and while its menu is open, 4px radius inset 2px from
  the bar's top and bottom. Text is Body (14/20); the application name is Body
  Strong. Icons are 16px. Tray and status icons are single-colour symbolic
  icons (`DESIGN.md`, Icons) where the program gives a symbolic name; the
  Better OS mark is the one coloured picture Better OS adds.
- **Order**, leading to trailing: Better OS menu, application name, then a
  flexible gap, then tray, input method, network, sound, battery, clock,
  notifications. Nothing is centred, and nothing sits between the application
  name and the gap: the bar never shows an application's own menus (D5).
- **Menus and flyouts** drop from the bar: menus are `fluent::menu` (8px
  radius, flyout fill, shadow), their top edge 4px under the bar, their
  leading edge aligned with the item's leading edge (trailing edge for items on
  the right side, so they never leave the screen). Flyouts (network, sound,
  battery, clock and notifications, tray overflow) are 360px wide.
- **One open at a time.** While a menu or flyout is open, moving the pointer to
  another item opens that one instead (macOS behaviour); a click outside, a
  second click on its item, or Escape closes it (DESIGN.md's light-dismiss
  rule).

## 2. Shared states

| State | What the bar shows | zh-TW | English |
| --- | --- | --- | --- |
| Starting (service up, nothing read yet, under 300 ms) | the Better OS menu and the clock only | — | — |
| A part still reading after 300 ms | that item shows its icon at 36% opacity; its tooltip says so | 正在讀取… | Reading… |
| A part's source is not there (no NetworkManager, no UPower, no battery, no audio server) | the item is not drawn | — | — |
| A part's source failed to answer | the icon with a small caution mark; its flyout says what failed and offers one action | 無法讀取{part}的狀態。 / 重試 | {part}'s state can't be read. / Try again |
| The shell adapter is missing or older than this bar | no bar is drawn (nothing can place it); Better Manager's Top bar card says so | 需要 Better OS 殼層轉接器才能顯示頂端列。 / 安裝 | The top bar needs the Better OS shell adapter. / Install |
| The shell adapter is off (person turned it off in Extensions) | no bar; the card says so with a button that turns it on through Better Manager | Better OS 殼層轉接器已關閉，頂端列無法顯示。 / 開啟 | The Better OS shell adapter is turned off, so the top bar can't show. / Turn on |
| GNOME other than 46 | no bar; the card says the desktop is not supported | 這個版本的 GNOME 還不支援頂端列。 | The top bar doesn't support this version of GNOME yet. |
| The view process ended unexpectedly | the service asks the shell to start it again after 1, 2, 4, then 30 seconds; after three failures in a minute it stops and Better Manager's Home shows an attention card | 頂端列一直無法啟動 / The top bar keeps failing to start · 查看 / View | |
| A fullscreen window on the bar's monitor | the bar is under the fullscreen window and its strip is not reserved (Mutter puts a dock in its bottom layer while the monitor is fullscreen); it comes back when the window leaves fullscreen | — | — |
| GNOME's overview or application grid is open | the overview covers the bar as it covers every window; it is back when the overview closes | — | — |
| Lock screen, login screen | no bar (the shell covers it; the view draws nothing new while locked) | — | — |

Part names for `{part}`: 網路 / the network, 音量 / sound, 電池 / the battery,
輸入法 / the input method, 系統匣 / the tray, 通知 / notifications.

## 3. The Better OS menu

The Better OS mark, 16px, at the bar's leading edge. Its menu:

| Item | zh-TW | English | What it does | Shown when |
| --- | --- | --- | --- | --- |
| About this computer | 關於這台電腦 | About This Computer | `better-manager --page system`, scrolled to 關於這台電腦 / About this PC | always |
| Control center | 控制中心… | Control Center… | `better-manager` | always |
| (separator) | | | | |
| Force quit | 強制結束應用程式… | Force Quit Applications… | opens Better Monitor on its Apps view | Better Monitor installed |
| (separator) | | | | |
| Sleep | 睡眠 | Sleep | `org.freedesktop.login1.Manager.Suspend(true)`, interactive | `CanSuspend` is `yes` or `challenge` |
| Restart | 重新啟動… | Restart… | `org.gnome.SessionManager.Reboot()`: GNOME's own end-session dialog with its countdown | always |
| Shut down | 關機… | Shut Down… | `org.gnome.SessionManager.Shutdown()`, the same dialog | always |
| (separator) | | | | |
| Lock screen | 鎖定畫面 | Lock Screen (shows GNOME's own `screensaver` key, `Super+L` on the machine) | `org.gnome.ScreenSaver.Lock()` | `org.gnome.desktop.lockdown disable-lock-screen` is false |
| Log out | 登出 {name}… | Log Out {name}… | `org.gnome.SessionManager.Logout(0)`: GNOME's dialog | `disable-log-out` is false |

`{name}` is the person's full name from `org.freedesktop.Accounts`, or the
login name when it is empty. Restart, Shut down, and Log out go through
GNOME's session manager so its "other people are logged in" and "unsaved
work" handling and its 60-second countdown are the desktop's own; the menu
never shuts down by itself. Sleep is grayed out with the reason in a tooltip
when `CanSuspend` is `na`: 這台電腦目前無法睡眠。 / This computer can't sleep
right now.

A method that fails (the session manager is not answering) shows a toast
through the bar's `ToastHost`: 無法{action}：{reason} / Couldn't {action}:
{reason}, with actions 重新啟動 / restart, 關機 / shut down, 登出 / log out,
鎖定 / lock, 睡眠 / sleep.

Accessible name of the button: Better OS 選單 / Better OS menu.

## 4. The application name and the actions every window has

The focused window's application name in Body Strong, as GNOME names it
(`Shell.WindowTracker`'s application for the window, from its desktop entry,
in the person's language). Truncated with an ellipsis at 200px. It is the same
for every application, Better OS's own included: the name, and a menu of the
actions every window has. What an application can do beyond that is in its
own window, on its own menu row or command bar, which the bar never hides or
repeats (D5).

| State | What is drawn |
| --- | --- |
| A normal window is focused | its application name |
| A window with no desktop entry | the window's `WM_CLASS` or Wayland `app_id`, as GNOME shows it in Alt+Tab |
| No window is focused (an empty desktop, every window minimised) | 桌面 / Desktop, whose menu has only 顯示所有視窗 / Show All Windows (the overview) |
| The focused window is the bar, its menus, a Better OS panel (Launcher), or a dock | nothing changes: the bar keeps showing the last normal window |
| A dialog of an application | the application of its parent window |

Its menu, the actions every window has:

| Item | zh-TW | English | Shortcut shown | What the shell does | Shown when |
| --- | --- | --- | --- | --- | --- |
| New window | 開新視窗 | New Window | — | `Shell.App.open_new_window(-1)` | the application's desktop entry says it can (`can_open_new_window`) |
| Full screen | 全螢幕 / 結束全螢幕 | Enter Full Screen / Exit Full Screen | — | `make_fullscreen()` / `unmake_fullscreen()` | the window allows it |
| Minimise | 最小化 | Minimize | — | `minimize()` | the window allows it |
| (separator) | | | | | |
| Close window | 關閉視窗 | Close Window | Alt+F4 | `delete(time)` | the window allows it |
| Quit | 結束 {app} | Quit {app} | — | `Shell.App.request_quit()` | always |

The shortcut column shows only shortcuts GNOME itself binds (`close` is
`<Alt>F4` on the machine); the bar never invents one, and it does not read a
window's own key map. An item the window does not allow is not drawn, never
drawn disabled, so the menu is never a list of things that cannot be done. A
Quit that the application answers by asking about unsaved work is the
application's own dialog; the bar does not wait for it.

Accessible name of the button: {app} 選單 / {app} menu.

## 5. Menus the bar draws

The bar draws three kinds of menu, all as `fluent::menu`: the Better OS menu
(section 3), the application name's menu (section 4), and a tray item's own
menu, which the item serves over `com.canonical.dbusmenu` and the bar reads as
a client (section 6.1). The bar serves no menu to anyone. The rules below hold
for all three; the third is the only one whose contents come from another
program, so most of them exist for it.

- Items are `fluent::menu` rows: an icon slot (16px) only when any item in the
  menu has an icon or a check, the label, the shortcut in secondary text at the
  trailing edge, a chevron for a submenu. Separators are 1px card stroke with
  4px above and below. A check item shows a check glyph; a radio item a dot.
- A label longer than 360px is truncated with an ellipsis; its full text is the
  tooltip and the accessible name.
- An item a tray item marks hidden is not drawn; one it marks disabled is drawn
  in disabled text. A submenu that is empty is drawn disabled.
- Mnemonics (`_Quit`) are not shown as underlines; the character is used for
  type-to-select inside an open menu.
- A tray item's menu changes while it is open when the item sends
  `LayoutUpdated` or `ItemsPropertiesUpdated`; the bar sends `AboutToShow`
  before opening a submenu and `Event("clicked")` when an item is chosen.
- A tray menu whose item does not answer within 500 ms opens with one disabled
  row: 這個應用程式沒有回應。 / This app isn't responding. The bar never waits
  longer than that before opening something.

## 6. The right side

### 6.1 The tray

While the bar is on, every StatusNotifierItem shows here and not in the bottom
taskbar (owner, 2026-11-12), and so does every old X11 (XEmbed) tray icon,
through the bridge (D6). The bar service is the `StatusNotifierHost`, and
the `StatusNotifierWatcher` while it is on, because the watcher on the machine
today is Zorin's AppIndicator extension, and that extension must be off for
its icons to leave the taskbar. With it off, nothing holds the X11 tray
selection, and Better OS's bridge takes it (section 9.6).

**The switch is one Better Defaults entry**, "Tray icons in the top bar",
applied when the bar is turned on and restored when it is turned off or
removed, with Better Defaults' capture, apply, verify, and restore:

| Desktop | Applied | Restored |
| --- | --- | --- |
| Zorin OS (`zorin-appindicator@zorinos.com`) | the extension is taken out of `org.gnome.shell enabled-extensions`, its previous presence captured | put back exactly where it was in the list, if it was there |
| Ubuntu (`ubuntu-appindicators@ubuntu.com`) | the same | the same |
| Neither present | nothing to change; the entry reports "nothing to do" | — |

The order matters and is the service's: on turning on, it takes
`org.kde.StatusNotifierWatcher` the moment the extension releases it, and the
bridge then takes the X11 tray selection; on turning off, the bridge releases
the selection and the service releases the watcher name before the extension
is put back. Items that follow the specification watch the watcher's name and
register again with the new owner (Better OS's own trays do since tickets 97
and 102; that libappindicator and the other toolkits' items do is not verified
here, and ticket 156 checks it). XEmbed programs that follow the system tray
specification watch for the `MANAGER` message the new selection owner sends
and dock again. An item that does neither is missing until it restarts; the
bar's settings card lists how many items were registered before the switch and
how many came back, so a missing one is visible: 有 {n} 個系統匣圖示沒有回來，重新開啟那個應用程式就會出現。 /
{n} tray icons didn't come back; reopening that app brings it back.

Icons: `IconName` from the icon theme, or `IconPixmap` scaled to 16 logical
pixels (32 at scale 2, picking the nearest size the item offers), with the
item's `Status`: `Passive` items go to the overflow, `NeedsAttention` shows
`AttentionIconName` or the pixmap and a dot. Clicking:

| Input | An item with a `Menu` | An item without one (every bridged XEmbed icon) |
| --- | --- | --- |
| Primary button | `Activate(x, y)` when `ItemIsMenu` is false, else opens its menu (section 5) | `Activate(x, y)` |
| Secondary button | opens its menu | `ContextMenu(x, y)`: the program draws its own menu |
| Middle button | `SecondaryActivate(x, y)` | `SecondaryActivate(x, y)` |
| Scroll | `Scroll(delta, orientation)` | the same |

`x` and `y` are the item's position in logical screen coordinates, the bottom
edge of the item, so a menu the program draws itself opens under the bar.
Tooltip: `ToolTip`'s title, else `Title`. Accessible name: the same text, then
系統匣 / tray.

A bridged XEmbed icon is an ordinary item to the bar: it is drawn, ordered,
and moved to the overflow like any other, and the bar does not know it is
bridged. It differs in what the program can offer, which section 9.6 lists:
its picture is a copy of what the program drew, so it is a coloured icon
rather than a symbolic one; it is never `Passive` or `NeedsAttention`; its
tooltip is its window's name; and its menu, when it has one, is the program's
own X11 menu, drawn by the program in its own style, not a `fluent::menu`.

**Overflow.** At most six items are shown on the bar (four below 1280px wide);
the rest, and every `Passive` item, sit behind a `▴` chevron that opens a
flyout: a grid of 48 × 40px cells, six per row, each the item's icon with its
title under it in Caption, truncated to the cell. Which items stay on the bar:
the ones the person dragged there (order is kept by item id in the service's
own state, `$XDG_STATE_HOME/better-os/top-bar/tray.json`), then registration
order.

| | zh-TW | English |
| --- | --- | --- |
| Chevron accessible name | 顯示隱藏的圖示 | Show hidden icons |
| Overflow flyout title | 系統匣 | Tray |
| Overflow hint (Caption) | 拖曳圖示到頂端列就會固定顯示。 | Drag an icon to the top bar to keep it there. |
| Overflow empty | 沒有其他圖示。 | No other icons. |
| No items at all | the chevron and the area are not drawn | — |

**Old X11 icons: what the person sees in each state of the bridge.** The bar
itself shows nothing about the bridge; the Top bar card (section 12) says what
is wrong when an XEmbed icon cannot be shown.

| Bridge state | The bar | The Top bar card (zh-TW / English) |
| --- | --- | --- |
| Holding the selection, XWayland running | every docked icon, as an item | — |
| XWayland not started yet (it starts on demand, when the first X11 program opens) | nothing to show: no X11 program runs, so no XEmbed icon exists | — |
| This desktop has no XWayland | — | 這個桌面沒有 X11 相容層（XWayland），所以不會有舊式的 X11 系統匣圖示。 / This desktop has no X11 compatibility layer (XWayland), so there are no old X11 tray icons. |
| Another program holds the X11 tray selection | no XEmbed icon is shown; the bridge takes the selection the moment that program lets it go | 另一個程式正在顯示舊式的 X11 系統匣圖示，所以這些圖示不會出現在頂端列。 / Another program is showing old X11 tray icons, so they won't appear in the top bar. |
| XWayland ended (it stops when its last X11 program closes, or it crashed) | the bridged items leave; they come back when XWayland and the programs start again | — |
| The bridge's unit failed | the bridged items leave | 無法顯示舊式的 X11 系統匣圖示。 / 重試 · Old X11 tray icons can't be shown. / Try again |

### 6.2 Input method

Shown when the person has more than one input source, or one ibus engine
(the machine has one: `('ibus', 'chewing')`). The label is the source's short
name as GNOME's keyboard indicator draws it, which for an ibus engine is the
engine's `<symbol>` (`酷` for Chewing: `/usr/share/ibus/component/chewing.xml`
has `&#x9177;`) and for a layout its short name (`en`), in Body. GNOME Shell is
the ibus panel on the machine (`ibus-daemon --panel disable`), so the shell
keeps owning the input method; the bar reports and switches through the panel
relay:

- the list of sources and the current one, as the shell's
  `InputSourceManager` has them, reported by the relay;
- switching calls the source's own `activate()` in the shell;
- the engine's own menu (Chewing's 中/英 and 全形/半形 switches) is the ibus
  property list GNOME's keyboard indicator already holds, relayed the same
  way, and activated by index against the shell's current list;
- while the bar shows the input method, the relay hides GNOME's own keyboard
  indicator (`SuppressKeyboardIndicator`), so it is not shown twice; it comes
  back when the bar goes.

Flyout (a menu): one radio item per source, then the engine's properties, then
輸入法設定… / Input Method Settings… (`gnome-control-center keyboard`).
Accessible name: 輸入法：{source} / Input method: {source}. When ticket 99's
Better Input exists, its own state appears here through the same list; nothing
in the bar changes for it.

### 6.3 Network

Read from NetworkManager on the system bus (unprivileged). Icon by the primary
connection: Wi-Fi with signal bars (0–4 from `Strength`), wired, VPN badge,
airplane mode, disconnected. A flyout:

| Element | zh-TW | English |
| --- | --- | --- |
| Wi-Fi toggle | Wi-Fi | Wi-Fi |
| Network list | one row per access point, strongest first, the connected one first with 已連線 / Connected; a lock for a secured one | |
| Connecting | 正在連線… | Connecting… |
| Needs a password | GNOME's own secret prompt opens (NetworkManager asks the session's agent, GNOME Shell); the flyout says 請在對話框輸入密碼。 | Enter the password in the dialog. |
| Failed | 無法連線到「{ssid}」。 | Couldn't connect to "{ssid}". |
| Airplane mode | 飛航模式 | Airplane mode |
| VPN rows | one per VPN connection with a toggle | |
| Settings | 網路設定… | Network Settings… |
| No Wi-Fi device | the Wi-Fi toggle and list are not drawn | — |
| NetworkManager absent | the item is not drawn | — |

Turning Wi-Fi on or off and activating a known connection use
NetworkManager's own polkit actions, which allow the active session on Ubuntu;
a connection that needs a new secret goes to GNOME's agent. Nothing goes
through `manager-daemon`. Tooltip and accessible name: 網路：{state} /
Network: {state}, with states 已連線到 {ssid} / Connected to {ssid}, 有線網路 /
Wired, 未連線 / Not connected, 飛航模式 / Airplane mode.

### 6.4 Sound

Read and written through the PulseAudio protocol that `pipewire-pulse` serves
on the machine. Icon by output volume (muted, low, medium, high). Scrolling on
the icon changes volume by 5%. Flyout:

| Element | zh-TW | English |
| --- | --- | --- |
| Output slider | 輸出 | Output |
| Mute | 靜音 / 取消靜音 | Mute / Unmute |
| Output device choice | 輸出裝置 | Output device |
| Input slider and device (shown only while an input exists) | 輸入 · 輸入裝置 | Input · Input device |
| Settings | 音效設定… | Sound Settings… |
| No audio server | the item is not drawn | — |

The crate that speaks the protocol is ticket 157's choice, with a license
review in `engineering/specs/copyleft-review.md` if it links libpulse.
Accessible name: 音量 {n}% / Volume {n}%, or 已靜音 / Muted.

### 6.5 Battery

Shown only when UPower's display device is a battery. Icon and state words are
`hardware_core::charge_state::battery_look`'s, the rule every Better OS battery
icon follows (`engineering/specs/battery-charge-state.md`, ticket 170), so a
battery held at its charge limit shows the plug picture and 已接上電源，維持在
80%, never the on-battery icon. The rule reads the kernel's words; UPower's
display device states map onto them (`charging` Charging, `discharging`
Discharging, `fully-charged` Full, `pending-charge` Not charging, the rest
unknown), and the limit is `charge_control_end_threshold`, read as the System
page reads it. Icon by charge and state,
with the percentage after it in Body (`87%`). Flyout:

| Element | zh-TW | English |
| --- | --- | --- |
| Line 1 | {n}% · {state} | {n}% · {state} |
| States | 充電中 / 使用電池 / 已接上電源，維持在 {limit}% / 已接上電源，已充滿 / 已接上電源，沒有充電 / 無法判讀電池狀態 | Charging / On battery / Plugged in, holding at {limit}% / Plugged in, fully charged / Plugged in, not charging / Battery state unknown |
| Time | 約 {time} 後充滿 / 約剩 {time} | About {time} until full / About {time} left |
| Power mode | the control center's Power mode `ComboBox` (same copy, `design/control-center-spec.md` section 4), when power-profiles-daemon is present | |
| Keep awake | the Keep awake toggle of Home's quick settings (same copy), when Better Awake is installed | |
| Settings | 電源設定… | Power Settings… → `better-manager --page system` |

Status words are the control center's (section 4 of that spec), so the two
never disagree. A time UPower reports as 0 is not shown, never guessed.
Accessible name: 電池 {n}%，{state} / Battery {n}%, {state}.

### 6.6 Clock

The date and time, by GNOME's own settings: `clock-format` (12h on the
machine), `clock-show-weekday`, `clock-show-date`, `clock-show-seconds`, read
live. Local time through ticket 146's one local-time function.

| Settings | zh-TW | English |
| --- | --- | --- |
| 12h, weekday, date | 11月21日 週六 下午3:42 | Sat Nov 21 3:42 PM |
| 24h, weekday, date | 11月21日 週六 15:42 | Sat Nov 21 15:42 |
| no date, no weekday | 下午3:42 | 3:42 PM |
| with seconds | 下午3:42:07 | 3:42:07 PM |
| Below 1280px (section 8) | 下午3:42 | 3:42 PM |

A clock with no time-zone data says so in its tooltip and shows UTC (ticket
146's rule): 找不到時區資料，顯示的是 UTC。 / No time-zone data; this is UTC.
The clock redraws on the minute (or second) boundary only.

### 6.7 Notifications and the calendar

The clock and the bell open one flyout (D7, shape a), 360px wide, the height of
the work area at most:

| Element | zh-TW | English |
| --- | --- | --- |
| Header | {date, long} | {date, long} |
| Calendar | a month grid, today in accent, weeks starting on the locale's first day; ‹ › change month | |
| Notifications header | 通知 | Notifications |
| Clear all | 全部清除 | Clear all |
| Do not disturb | 勿擾 (ToggleSwitch) | Do not disturb |
| A notification | app icon, app name and time in Caption, title in Body Strong, body in Body (three lines, then 顯示更多 / Show more), its action buttons, a close button | |
| Empty | 沒有新通知。 | No new notifications. |
| DND on, empty | 勿擾已開啟，不會跳出通知。 | Do not disturb is on. Notifications won't pop up. |
| Relay not available | 無法讀取通知。 | Notifications can't be read. |

The bell shows a dot while there are unread notifications, and a crossed bell
while Do not disturb is on. Do not disturb is GNOME's own
`org.gnome.desktop.notifications show-banners`, so GNOME's banners obey it.
Clicking a notification activates it in the shell (the shell's own
`activate()`, which opens the application) and closes the flyout; its close
button dismisses it in the shell; Clear all clears them in the shell. The
banners themselves stay GNOME's: the bar does not draw pop-ups.

The flyout lists what the shell reports; the bar service keeps the list in
memory only and writes no notification anywhere.

Ticket 161 (the owner's request of 2026-11-21 for a notification island at
the top right and a notification center on the right edge) may replace the
notification half of this flyout; ticket 158 waits for its design.

Accessible names: 通知，{n} 則未讀 / Notifications, {n} unread; 日期與時間：{date} /
Date and time: {date}.

## 7. Keyboard access and screen readers

| Key | What it does |
| --- | --- |
| Ctrl+F2 | focuses the bar: the Better OS menu is highlighted |
| Left, Right | move between items; with a menu open, open the neighbouring item's menu or flyout |
| Down, Enter, Space | open the highlighted item's menu or flyout; in a menu, Down and Up move, Right opens a submenu, Left closes it |
| type a letter | in an open menu, jump to the next item starting with it |
| Escape | close the menu; a second Escape returns focus to the window that had it |
| Super+V, Super+M | open the notifications flyout |

The bar's keys never reach an application: Ctrl+F2 focuses the bar, not a
window's menu row, and a window's own menu keys (F10, Alt) stay the window's.

Super+V and Super+M are GNOME's own keys for its calendar and notification
list (`org.gnome.shell.keybindings toggle-message-tray ['<Super>v',
'<Super>m']` on the machine). While the bar is on, Better Defaults moves them:
GNOME's `toggle-message-tray` is set to `[]` and two GNOME custom key bindings
take the same keys, and Ctrl+F2 (unbound on the machine) is a third. Turning
the bar off restores `toggle-message-tray` and removes the custom bindings,
with the changed-elsewhere rule ticket 94 uses. The custom bindings run
`better-top-bar --focus` and `better-top-bar --notifications`, which ask the
running service over its bus name. `<Super>n` stays GNOME's
`focus-active-notification`, which acts on GNOME's banner. A dock does not
take focus by itself; the shell focuses the bar's window when the service asks
(the panel relay's `FocusView`), and gives focus back to the window that had it
when the menu closes.

Every item, menu, and row has an accessible name (the tables above) and a role
(tool bar for the bar; button with a menu for the Better OS mark, the
application name, and a tray item with a menu; button for the tray items
without one; toggle button; menu and menu item, check menu item, radio menu
item inside menus), exposed through GPUI's AccessKit adapter. The bar is named
頂端列 / Top bar. It is not a menu bar to a screen reader, because it holds no
application's menus: a screen reader that offers "go to the menu bar" goes to
the window's own. A bridged XEmbed icon's accessible name is its window's name,
then 系統匣 / tray, like any item; what the program's own X11 menu tells a
screen reader is the program's. Status changes the person should hear (battery
low at 10%, a network lost) are announced politely, never more than once per
change.

Focus is drawn with `DESIGN.md`'s focus visual, only when it came from the
keyboard.

## 8. Sizes, scale, monitors, direction

**Width.** As the bar narrows, things give way in this order, so the Better
OS menu, the application name, and the clock's time are always visible:

1. Below 1280 logical px: the clock drops the date and weekday; at most four
   tray items.
2. Below 1024: the battery drops its percentage text; tray items all go into
   the overflow.
3. Below 960: the application name truncates to 120px. (The machine's own
   display is 1440 logical px wide at scale 2; 960 is the narrowest width
   Better OS windows are designed for, and a display set to a larger scale or a
   small external monitor reaches it.)

The mockup `rest-960-*.html` draws the 960px bar.

**Scale.** The bar is laid out in logical pixels; at scale 2 everything is
twice as many physical pixels and drawn sharp (GPUI renders at the output's
scale). Fractional scale through `wp_fractional_scale_manager_v1`, which the
compositor offers. Mockups are drawn at scale 1. A bridged XEmbed icon is a
picture the program drew in X11 pixels; section 9.6 says how its size and
scale are handled.

**Monitors.** The bar is on the primary monitor (Mutter's primary, the one
with GNOME's top panel). A setting, 在所有螢幕顯示頂端列 / Show the top bar on
every display, off by default, draws one bar per monitor; each shows the name
and actions of the focused window, as macOS does. A monitor connected or
removed, or the primary changed, moves or adds the bar within a second (the
shell reports `monitors-changed`). A monitor with its own scale gets a bar
drawn at that scale.

**Right to left.** Not supported in this release: Better OS ships zh-TW and
English only (`engineering/specs/language-policy.md`). The layout is described
in leading and trailing terms so mirroring it later changes no rule.

**Under a maximized window.** The bar's strip is reserved (section 9.3), so a
maximized or tiled window starts under the bar and never covers it; Mutter
keeps a window's title bar inside the work area when it is dragged.

**GNOME's own top panel.** On Zorin OS (the owner's machine) GNOME's panel is
inside Zorin's bottom taskbar, so nothing is at the top until the bar is. On
Ubuntu, or on Zorin with "keep the top panel", GNOME's panel is at the top: the
relay hides it, and its reserved strip with it, while the bar's service owns
its name, and shows it again the moment the name goes, the extension is
disabled, or the bar is removed. Nothing is written to any setting for that.

## 9. How it is built on GNOME 46

### 9.1 Nothing but GNOME Shell can draw a panel

- **No layer-shell.** Mutter 46 on the machine advertises no
  `zwlr_layer_shell_v1` or `ext_layer_shell` (the registry list in the sources),
  and none is compiled into it. Mutter's maintainers do not intend to support
  third-party panels (issue 1053). GPUI's `WindowKind::LayerShell` is refused
  on GNOME.
- **A normal window is not a panel.** A toplevel takes focus when clicked,
  shows in the taskbar, Alt+Tab, and the overview, can be moved, and reserves
  no space; a Wayland client cannot place itself.
- **An XWayland dock** (`_NET_WM_WINDOW_TYPE_DOCK` with struts) is the one
  route that needs no shell code, and is rejected: it ties the bar to X11
  inside a Wayland session, GPUI would run its X11 backend for this one
  window, and on a display at scale 2 an X11 window's sharpness and placement
  depend on XWayland's scaling mode, which `experimental-features` controls
  and which Better OS must not change. It was not tried on the machine. (The
  XEmbed bridge of section 9.6 is an X11 client too, but it draws nothing on
  screen: it only reads pictures.)

So the shell has to take part. The question is how little it does.

### 9.2 What GNOME Shell offers an extension

From the library and source above: an extension can start a program through
`Meta.WaylandClient` (`spawnv`), and for a window of that client only,
`make_dock(window)` turns it into a dock: not focused on click, not in the
taskbar, pager, or Alt+Tab, on every workspace, in the dock layer, and below a
fullscreen window. It can place any window (`move_frame`), reserve a strip of
the screen with an invisible chrome actor (`Main.layoutManager.addChrome(actor,
{affectsStruts: true, trackFullscreen: true})`, as Zorin's taskbar does at
`panelManager.js:723-726`), focus a window, and read every window's
application and PID. Zorin's own desktop icons
extension uses the same route for its desktop window
(`zorin-desktop-icons@zorinos.com/extension.js:511-525`,
`emulateX11WindowType.js:216-232`). On GNOME 46 a client can become a dock
only if the shell started it: `new_indirect` and `setup_fd`, which would let an
outside process connect as such a client, are not callable from GJS.

### 9.3 The shape: a Better OS window the shell makes a dock (D1 A)

Three Better OS processes, and the relay inside the shell:

```
 better-top-bar.service (user unit, Rust)          the shell (GJS relay)            better-top-bar-view (GPUI)
 ───────────────────────────────────────          ─────────────────────            ──────────────────────────
 owns org.betteros.TopBar1                 ──► StartView() ──► spawnv(fixed path) ──► connects through
 decides everything: layout, menus,                make_dock, move_frame,              WAYLAND_SOCKET, draws
 tray host + watcher,                      ◄── FocusChanged, Monitors, Fullscreen    the bar and its menus
 network, sound, battery, clock,           ──► ReserveStrip(monitor, edge, px)        (xdg_popup), talks only
 notifications list                        ◄── Notification*, InputSources            to the service
        ▲                                  ──► WindowAction(id, action)
        │ StatusNotifierItem, one bus connection per icon (the same protocol as any tray item)
 better-top-bar-xembed.service (user unit, Rust) ◄──► XWayland: the _NET_SYSTEM_TRAY_S0 selection, docked icon windows
```

- **`better-top-bar`** (crate `top-bar-service`, a systemd user unit in the
  `better-top-bar` package, enabled by installing like every service). It owns
  `org.betteros.TopBar1`, is the StatusNotifierHost and Watcher (section 6.1),
  reads NetworkManager, UPower, the audio server, and the clock settings, holds
  the notification list, and decides every state in this file in
  `top-bar-core`, a crate with no GPUI dependency and with the tests.
- **`better-top-bar-view`** (crate `top-bar-gui`), at
  `/usr/libexec/better-os/top-bar-view`, started by the shell and by nothing
  else. It draws what the service tells it, as a GPUI window on the shell's
  Wayland client connection, and menus and flyouts as `AnchoredPopup`
  (`xdg_popup`) children of it. It mounts `UpdateRestart`, but restarting in
  place would lose the shell's connection, so when its program is replaced it
  exits instead and the service asks the shell for a new one (AGENTS.md's
  restart rule is met: the new program runs within seconds).
- **`better-top-bar-xembed`** (crate `top-bar-xembed`), at
  `/usr/libexec/better-os/top-bar-xembed`, its own user unit in the same
  package: the XEmbed bridge (section 9.6). It speaks only StatusNotifierItem
  to the service, exactly as any tray program does, so the service has no
  XEmbed code.
- **The panel relay** in `shell-adapter@betteros.org`, under ADR 0036: active
  only while `/usr/libexec/better-top-bar` owns `org.betteros.TopBar1`. It
  starts the view on request, makes its window a dock and places it, reserves
  the strip, hides GNOME's top panel, reports what the bar needs to know, and
  performs window and input-source actions. It decides nothing and stores
  nothing but handles.

### 9.4 The panel relay's interface

`org.betteros.ShellAdapter1.Panels` at `/org/betteros/ShellAdapter1/Panels`;
counterpart `org.betteros.TopBar1`. No argument of any method is a string.

| Member | Shape | What it is |
| --- | --- | --- |
| `StartView()` | | Starts `/usr/libexec/better-os/top-bar-view` through `Meta.WaylandClient` with no arguments, if the relay has no running view. Refused while one runs. |
| `ViewStarted(t window)` signal | | The view's first window mapped and was made a dock. |
| `ViewEnded(i status)` signal | | The view process exited. |
| `PlaceView(t window, i x, i y)` | | `move_frame(true, x, y)` on a window of the view's client; refused for any other window. |
| `ReserveStrip(u monitor, u edge, u thickness)` | | Adds or moves one invisible chrome actor that reserves `thickness` logical px along `edge` (0 top, 1 bottom) of that monitor, tracked fullscreen; `thickness` 0 removes it. At most one per monitor and edge. |
| `SuppressBuiltInPanel(b)` | | Hides GNOME's `Main.layoutManager.panelBox` while true, and shows it again on false, on the counterpart leaving, and on `disable()`. Refused when the panel is not where GNOME 46 keeps it. |
| `SuppressKeyboardIndicator(b)` | | Hides GNOME's `Main.panel.statusArea.keyboard` container while true and shows it again on false, on the counterpart leaving, and on `disable()`. |
| `FocusView(t window)` / `ActivateWindow(t window)` | | Focus the view's window; give focus back to a window (`Main.activateWindow`). |
| `WindowAction(t window, u action)` | | 0 minimize, 1 fullscreen, 2 leave fullscreen, 3 close (`delete`), 4 open a new window of its application, 5 quit its application (`request_quit`). |
| `ShowOverview()` | | `Main.overview.show()`. |
| `FocusChanged(t window, s app_id, s app_name, u pid, u client_type, u flags)` signal | | The focused window as the shell sees it; `flags` bits: can minimize, can fullscreen, is fullscreen, can close, can open new window, belongs to the view's client (`owns_window`); whether a window is a Better OS panel is decided in Rust from its `app_id`. Empty strings and 0 where a value does not exist. It carries no X11 window id and no GTK menu paths: those were for application menus, which D5 removed (ADR 0036's amendment). |
| `Monitors(a(uiiiidb))` signal | | Index, logical x, y, width, height, scale, is primary, for every monitor, on `monitors-changed`. |
| `FullscreenChanged(u monitor, b)` signal | | |
| `InputSources(a(uss) sources, u current)` signal, `ActivateInputSource(u index)` | | GNOME's input sources as the shell's `InputSourceManager` has them (index, id, short name); the engine's properties as GNOME's keyboard indicator holds them with a serial number; `ActivateInputProperty(u serial, u index)` resolves the index against the shell's current list at call time and is refused when the serial is stale. |
| `Notification(t id, s app, s title, s body, x time, u urgency, as action_labels)` signal, `NotificationRemoved(t id)`, `ListNotifications()`, `ActivateNotification(t id)`, `NotificationAction(t id, u index)`, `DismissNotification(t id)`, `ClearNotifications()` | | GNOME's message tray, copied unchanged, sent to the counterpart only. |
| `Capabilities()` | out `s` | JSON: protocol version, shell version, whether the view runs, the strips held, GNOME's panel found and hidden. |

Every method checks the caller against the counterpart's unique name and the
counterpart's executable, as ADR 0018 does; every signal goes to the
counterpart only. The XEmbed bridge needs nothing from the relay.

### 9.5 Shape B, for comparison (D1 B, refused)

Zorin's taskbar puts GNOME's panel back at the top when its
`stockgs-keep-top-panel` is true (`panelManager.js:67`), and then every
AppIndicator icon, the keyboard indicator, quick settings, and the date menu
are in that top panel with no Better OS code. Better Defaults would write that
one setting, plus `panel-element-positions` to hide the bottom panel's own
copies. What is missing is the left side: GNOME 46 has no application name
button with window actions in its panel, so the extension would add a Better
OS menu button and an application button and draw their menus (`PopupMenu`)
from trees the Rust service sends, keeping the tree and the labels in
JavaScript. It is fewer tickets and less risk on the right side, and it is not
Fluent; ADR 0036 records why it was not chosen.

### 9.6 The XEmbed bridge (D6)

Old X11 programs show a tray icon by the system tray specification: a program
looks for the owner of the X selection `_NET_SYSTEM_TRAY_S<screen>`, sends it
a `_NET_SYSTEM_TRAY_OPCODE` client message with `SYSTEM_TRAY_REQUEST_DOCK`,
and the owner embeds the program's small icon window with XEmbed. On a Wayland
session such a program runs on XWayland, and today Zorin's AppIndicator
extension owns the selection through GNOME Shell's `Shell.TrayManager`
(sources). The bridge takes the selection instead while the bar is on and
turns every docked icon into a StatusNotifierItem.

**How `xembedsniproxy` does it** (plasma-workspace `master`, read 2026-11-21,
cited by file and line; it is the model, not a dependency):

| Step | `xembedsniproxy` | Where |
| --- | --- | --- |
| Take the selection | `KSelectionOwner` on `_NET_SYSTEM_TRAY_S<n>` without forcing (`claim(false)`), and logs and gives up if another owner has it | `fdoselectionmanager.cpp:51-72`, `198-207` |
| Offer an alpha visual | sets `_NET_SYSTEM_TRAY_VISUAL` on the owner window to a 32-bit TrueColor visual when the screen has one | `fdoselectionmanager.cpp:209-213`, `xcbutils.h:122-165` |
| Accept a dock request | `SYSTEM_TRAY_REQUEST_DOCK` in a `_NET_SYSTEM_TRAY_OPCODE` client message | `fdoselectionmanager.cpp:128-135` |
| Hold the icon window | creates an override-redirect 32 × 32 container (`s_embedSize = 32`), sets its opacity to 0 and its input region empty, reparents the icon window into it, sends `XEMBED_EMBEDDED_NOTIFY`, and adds the icon to its save-set so the icon goes back to the root window if the proxy dies | `sniproxy.cpp:49`, `117-177` |
| Keep the picture readable | `composite_redirect_window(…, MANUAL)` on the icon window, so it renders off screen | `sniproxy.cpp:166-169` |
| Notice a change | one `Damage` object per icon (`REPORT_LEVEL_NON_EMPTY`); each `DamageNotify` triggers a new capture and a `DamageSubtract`. The first paint sends no damage, so one capture is taken 500 ms after docking | `fdoselectionmanager.cpp:55-66`, `83-92`, `147-153`; `sniproxy.cpp:206-209` |
| Capture | `GetImage` of the icon window, converted by depth (24, 30, 32; 16 and 1); a 24-bit picture gets a mask from its corner colour; a fully transparent capture is skipped as a known X11 glitch; anything larger than 32 × 32 is scaled down keeping its aspect | `sniproxy.cpp:216-235`, `305-393` |
| Resize | an icon window that asks to resize is allowed up to 32 × 32; moves are ignored | `fdoselectionmanager.cpp:155-165`, `sniproxy.cpp:238-271` |
| Undock | on `UnmapNotify` or `DestroyNotify` of the icon window | `fdoselectionmanager.cpp:137-146` |
| Be an item | one session-bus connection per icon, because a watcher cannot see one object of a shared connection go away; `Category` `ApplicationStatus`, `Status` `Active`, `ItemIsMenu` false, `Title` from the window's name, `Id` the title or the window id, the picture as `IconPixmap` | `sniproxy.cpp:92-96`, `471`, `490-531` |
| Forward input | `Activate` → button 1, `SecondaryActivate` → button 2, `ContextMenu` → button 3, `Scroll` → buttons 4/5 (vertical) or 6/7 (horizontal); the container is moved under the click point, made clickable for 300 ms, and the press and release are sent as X events when the icon selected button events, else through XTest; on Wayland it also warps the pointer into the icon window. A Wine icon shaped with the Shape extension is clicked at its first shaped point | `sniproxy.cpp:194-204`, `397-466`, `534-662`; `xtestsender.cpp` |

**What Better OS's bridge is.** `top-bar-xembed`, a Rust program, its own
systemd user unit `better-top-bar-xembed.service` in the `better-top-bar`
package, at `/usr/libexec/better-os/top-bar-xembed`. It does what the table
above does, in Rust, with these choices:

- **Its own unit, not part of `better-top-bar`.** An X11 connection is a
  failure the service should not share: a hung XWayland or a misbehaving icon
  window must not stop the tray host, the clock, or the notifications. And it
  needs nothing from the service but the StatusNotifierItem protocol, so the
  service stays free of X11. Under AGENTS.md's unit rules it is a user unit
  like the service: dpkg's file list names it, `manager-core` enables and
  starts it on install and enable, stops and disables it on disable and
  removal, and the startup pass starts it again if someone stopped it by hand;
  the package's postinst `try-restart`s it after an update. It is `PartOf` and
  `After` `better-top-bar.service`, so it stops and restarts with the bar.
- **It never starts XWayland.** On GNOME 46, connecting to the X socket is
  what starts XWayland on demand (sources: the socket exists while no
  `Xwayland` runs), and a bridge that connected at login would start an X
  server on every login for nothing. So the bridge stays running and connected
  to nothing X11 until XWayland is up: it watches the user manager's
  `gnome-session-x11-services.target` over `org.freedesktop.systemd1`
  (subscribed to that unit's `PropertiesChanged`, no timer), and when the
  target becomes active it reads `DISPLAY` from the user manager's
  `Environment` and connects. When the target stops, or the X connection
  closes, it drops every item and waits again. A desktop where that target does
  not exist (`LoadState` `not-found`) and `DISPLAY` is unset has no XWayland:
  the bridge reports that state (section 6.1) and waits for the target. The
  unit is `active` in every one of these states, which is what the health
  check asks of an enabled unit.
- **The selection is taken only while the bar's watcher is up.** It claims
  `_NET_SYSTEM_TRAY_S0` (the screen number from `DISPLAY`) with
  `SetSelectionOwner` and then checks `GetSelectionOwner`, never forcing it
  from another owner, and announces itself with the ICCCM `MANAGER` client
  message on the root window so waiting programs dock. While another program
  owns it, the bridge selects XFixes selection-owner events for it and claims
  it the moment it is released (no polling). It releases the selection, and
  undocks every icon back to the root window, when
  `org.kde.StatusNotifierWatcher` or `org.betteros.TopBar1` loses its owner,
  on `SelectionClear` (another program took it: the card says so), and before
  it exits. The unit stops before the bar's service puts Zorin's extension back
  (section 6.1), so the two never contend.
- **X11 from Rust: `x11rb`.** `x11rb 0.13.2` is already in `Cargo.lock`
  through GPUI's X11 backend, under `MIT OR Apache-2.0`. The bridge uses its
  pure-Rust `RustConnection` (no libxcb linked) with the `composite`,
  `damage`, `xfixes`, `shape`, and `xtest` features, which only enable modules
  of `x11rb-protocol` that is in the lock too; no new crate is expected, and
  ticket 149 confirms the lockfile and the licence inventory are unchanged
  or regenerates them. The `xcb` crate also in the lock links libxcb and is not
  used.
- **Change is noticed by Damage, never by polling.** One `Damage` object per
  docked icon window at `NonEmpty`; a `DamageNotify` marks the icon dirty, and
  the bridge captures it with `GetImage` at most once per 100 ms per icon
  (damage inside that window is merged into one capture), then
  `DamageSubtract`s. One capture 500 ms after docking covers the first paint,
  as `xembedsniproxy` does. Idle cost: no timer runs and no request is sent
  while no icon changes; the process sleeps in its event loop. A capture is
  `32 × 32 × 4` = 4 KiB read and one `NewIcon` signal; an icon animating
  continuously costs at most ten of each per second, which ticket 149
  measures.
- **Size and scale 2.** The container is 32 × 32 X pixels, and the bridge
  announces `_NET_SYSTEM_TRAY_ORIENTATION` horizontal and
  `_NET_SYSTEM_TRAY_ICON_SIZE` 32 so toolkits that read them draw at that
  size. The picture offered is the size the icon window has, at most 32 × 32,
  larger scaled down keeping its aspect. The bar's own rule (section 6.1)
  picks the pixmap nearest to 32 physical pixels at scale 2 and draws it at 16
  logical pixels, so at scale 2 a 32-pixel picture is drawn one to one, and
  at scale 1 it is halved. With `experimental-features` `[]`, as on the
  machine, Mutter lays monitors out in physical pixels and XWayland's
  coordinates are physical pixels too; with `scale-monitor-framebuffer` they
  are logical. The bridge reads the layout mode from
  `org.gnome.Mutter.DisplayConfig.GetCurrentState`'s `layout-mode` and
  converts the logical `x`, `y` the bar sends into X root coordinates with the
  monitor's scale. That this conversion is right on the machine is the first
  thing ticket 149 checks.
- **Transparency.** The owner window's `_NET_SYSTEM_TRAY_VISUAL` names a
  32-bit TrueColor visual when the X screen has one, and the containers use
  it, so an icon that draws with alpha gives a picture with alpha; its
  premultiplied pixels are converted to the straight ARGB32, network byte
  order, that `IconPixmap` carries. An icon that draws on a 24-bit visual has
  no alpha: the container's background is the bar's own fill for the current
  appearance (`#f3f3f3` or `#202020`, changed when the appearance changes), so
  an icon with a `ParentRelative` background blends with the bar, and when
  all four corner pixels share one colour that colour is made transparent, as
  `xembedsniproxy` does. A capture that comes back fully transparent is
  ignored and the previous picture kept. Depths other than 1, 16, 24, 30, and
  32 are not converted and the icon is shown as missing (its title in the
  overflow, no picture).
- **What is forwarded.** `Activate`, `SecondaryActivate`, `ContextMenu`, and
  `Scroll` as buttons 1, 2, 3, and 4 to 7, as in the table above: the
  container is moved under the converted click point and made clickable for
  300 ms, and the press and release go as X events when the icon window or a
  child selected button events, else through XTest. The program's own X11
  menu opens where the program puts it and is drawn by the program; the bridge
  does not read, copy, or restyle it. Each item is on its own session-bus
  connection, for the reason in the table, and registers with
  `RegisterStatusNotifierItem` by its bus name.
- **What is not forwarded.** Keyboard input, pointer motion and hover (so a
  program's own hover tooltip never shows), drag and drop onto an icon, XEmbed
  focus and activation messages beyond `EMBEDDED_NOTIFY`, and any balloon
  message a program sends with `SYSTEM_TRAY_BEGIN_MESSAGE` (it is dropped;
  ticket 149 records which programs on the machine send one).
- **What an item says.** `Id` the icon window's `WM_CLASS` class, with `-2`,
  `-3` added for a second and third icon of the same class, so the person's
  drag order (section 6.1) survives a restart of the program; `Title` the
  window's `_NET_WM_NAME`, else `WM_NAME`, else the `WM_CLASS` class;
  `Category` `ApplicationStatus`; `Status` `Active`; `ItemIsMenu` false; no
  `Menu`. Nothing a program draws or names is written anywhere; the bridge
  keeps its items in memory only.

**Limits, stated plainly.**

- The bar shows a copy of a picture. It cannot make an XEmbed icon symbolic,
  recolour it for the dark appearance, or show it sharper than the program
  drew it; at scale 2 an icon that draws 16 X pixels is enlarged and soft.
- The menu of an XEmbed program is its own X11 menu, not a Fluent menu, and
  the bar's keyboard navigation and screen-reader names do not reach into it.
- Whether a forwarded click reaches a GTK 2 or GTK 3 icon on XWayland under
  Mutter is not known: the pointer is over the bar, a Wayland surface, not over
  any X window, and `xembedsniproxy`'s pointer warp is tested on KWin. Where the
  X server thinks the pointer is decides where a program's menu opens and
  whether its popup grab works. Ticket 149 tries it first and stops for the
  owner if it fails.
- A program that docks once at start and does not watch for `MANAGER` has no
  icon if it started before the bridge held the selection, until it restarts;
  the card's count of icons that did not come back (section 6.1) includes it.
- No attention state, no `Passive` state, and no tooltip beyond the window's
  name: XEmbed has none of them.
- No XWayland, no icons: on a session without it there is nothing to bridge,
  and on COSMIC the bridge applies only if ticket 143 finds XWayland there.

### 9.7 Why the bar shows no application menus (D5)

The first version of this design had the bar show the focused window's menus,
as macOS does. The owner decided against it (D5). The findings that design was
built on are kept here because they show what the decision gives up and what
it saves.

What exported a menu the bar could have shown on GNOME 46 Wayland:

| Toolkit | Exports a menu | Evidence |
| --- | --- | --- |
| Better OS (GPUI) | No: GPUI stores `set_menus` and exports nothing on Linux, and Better OS windows draw their own rows | `gpui_linux/src/linux/platform.rs:616-628`; `crates/git-gui/src/render.rs:583-960` |
| GTK 4 / libadwaita | Only a `GtkApplication` that sets a menubar, linked to its window through `gtk_shell1`; libadwaita applications normally have none | GTK `gtkapplication-wayland.c` (<https://raw.githubusercontent.com/GNOME/gtk/main/gtk/gtkapplication-wayland.c>) |
| GTK 3 | A `GtkApplication` with a menubar; any `GtkMenuBar` only with `appmenu-gtk-module`, not installed on the machine, and GTK 4 dropped module loading | the same file; KDE bug 424485 (<https://bugs.kde.org/show_bug.cgi?id=424485>) |
| Qt 5 / 6 | Only under XWayland through `com.canonical.AppMenu.Registrar`; on Wayland through `org_kde_kwin_appmenu`, which Mutter does not offer | Qt `qwaylandappmenu_p.h` (<https://codebrowser.dev/qt6/qtbase/src/plugins/platforms/wayland/qwaylandappmenu_p.h.html>), KDE review D3085 (<https://phabricator.kde.org/D3085?id=9146>) |
| Chromium, Electron | Only under XWayland through the registrar; on Wayland only `org_kde_kwin_appmenu` | Chromium `dbus_appmenu_registrar.cc` and `wayland_toplevel_window.cc` (<https://raw.githubusercontent.com/chromium/chromium/main/chrome/browser/ui/views/frame/dbus_appmenu_registrar.cc>, <https://raw.githubusercontent.com/chromium/chromium/main/ui/ozone/platform/wayland/host/wayland_toplevel_window.cc>) |
| Firefox | Off by default since 125, and its Wayland path needs a protocol Mutter 46 does not offer | Mozilla bug 1883184 (<https://bugzilla.mozilla.org/show_bug.cgi?id=1883184>) |

So on the owner's desktop the bar would have shown real menus for Better OS's
own windows (after building an export for them), a few GTK applications, and
X11 programs, and only the shared actions for the browsers, Electron
applications, libadwaita applications, and native Wayland Qt. What D5 gives
up is that mix; what it saves is three tickets of work and three standing
costs: owning `com.canonical.AppMenu.Registrar` and speaking `org.gtk.Menus`
to other programs, a menu model and dbusmenu exporter in `better-ui` that
every Better OS window would have to keep complete, and a window that hides
its own row while the bar has its menus and must show it again within a
second when the bar is gone. Ticket 95's premise that "GTK 4 applications have
no menu bar to export" was partly wrong (GTK 4 does link a `GtkApplication`
menubar through `gtk_shell1`), which did not change the outcome: few GNOME
applications set one.

The bar still reads `com.canonical.dbusmenu`, as a client, for one thing: a
tray item's menu (section 6.1). `awake-tray` and `monitor-tray` serve that
protocol for their own tray menus. Ticket 150 puts the server half they share
and the client half the bar needs in one crate.

## 10. The taskbar at the bottom

Today (owner's machine): Zorin's taskbar at the bottom, 45px, holding Zorin's
start menu, Show Applications, the window list, the tray, the system menu
(network, sound, battery, power), the clock, and the desktop button.

| | Zorin taskbar kept (D3 a, now) | Zorin taskbar as a dock (D3 b, refused) | Better OS taskbar (D3 c, once ticket 142 ships) |
| --- | --- | --- | --- |
| Top edge | the bar, 32px reserved | the bar | the bar |
| Bottom edge | Zorin's taskbar, 45px reserved | Zorin's taskbar, centred, intellihide on, nothing reserved | Better OS's taskbar, reserved by the same `ReserveStrip(monitor, 1, px)` |
| Clock, system menu, tray at the bottom | hidden through Zorin's `panel-element-positions` (`dateMenu`, `systemMenu` `visible: false`); `rightBox` stays, because it holds shell indicators the bar cannot draw (drive menu, printers, Zorin Connect), minus GNOME's keyboard indicator, which the relay hides while the bar shows the input method | hidden the same way | not drawn: the Better OS taskbar has none while the bar is on |
| Start / Better OS button | Zorin's | Zorin's | ticket 142's (D4: beside Show Applications until ticket 94, then in place of Zorin's) |
| Power, restart, lock, log out | the bar's Better OS menu and Zorin's menu | the same | the bar's Better OS menu only |
| What Better Defaults writes | one entry, two flags | one entry, five keys | none for Zorin's taskbar beyond ticket 142's "off while it runs" |

Rules that hold in all three:

- The two surfaces never overlap: the bar reserves the top strip and the
  taskbar the bottom one; flyouts from the bar never go below the work area's
  bottom.
- Turning the bar off restores every key the bar's entries wrote, and nothing
  else changes on the taskbar.
- Ticket 142's taskbar view uses the same panel relay (a second fixed program
  path in ADR 0036's closed table) and the same `ReserveStrip`, so one relay
  carries both.

## 11. On COSMIC

On 2026-11-21 the owner chose a Better OS session on `cosmic-comp` beside
GNOME (ADR 0038, option D; `engineering/specs/better-os-session.md`), with no
COSMIC panel in it, so the question of hiding COSMIC's panel does not arise.
The bar is built for that session first (ticket 152 on layer-shell, ticket
160 for the session's sources: focused window from `zcosmic_toplevel_info_v1`,
window actions through `zcosmic_toplevel_manager_v1`, the input method from
Fcitx 5, notifications from Better OS's own service, ticket 215), while the
GNOME relay of section 9 waits (ticket 151). Ticket 160 rewrites this section
in full; the paragraph below is the text from before the decision.

If ticket 143 makes COSMIC the desktop, the bar becomes simpler: cosmic-comp
offers layer-shell, on which COSMIC's own panel and its applets are built
(libcosmic's panel applet chapter), so `better-top-bar-view` would open its
window as GPUI's `WindowKind::LayerShell` anchored to the top edge with an
exclusive zone, and the panel relay is not needed. The tray host, network,
sound, battery, clock, and the bar's own menus are the same Rust, since none of
them depends on GNOME; the focused window, the input method, and the
notifications would need COSMIC's own sources instead of GNOME Shell's, and the
XEmbed bridge's way of noticing XWayland would need COSMIC's, which ticket
143's evaluation must name. Whether COSMIC's panel is turned off or kept is
that ticket's decision.

## 12. Better Manager: the Top bar card

The bar is a component, `better-top-bar`, installed, updated, turned off, and
removed through Better Manager. Its settings live on a control center card
under Settings › 頂端列 / Top bar:

| | zh-TW | English |
| --- | --- | --- |
| Header | 頂端列 | Top bar |
| Description | 螢幕上方的列：Better OS 選單、目前應用程式的視窗動作、系統匣、時鐘和通知。 | The bar at the top of the screen: the Better OS menu, the current app's window actions, tray, clock, and notifications. |
| Every display | 在所有螢幕顯示頂端列 | Show the top bar on every display |
| Tray note (Caption) | 頂端列開啟時，系統匣圖示（包括舊式的 X11 圖示）會從工作列移到這裡。 | While the top bar is on, tray icons, old X11 ones included, move here from the taskbar. |
| Icons that did not come back | 有 {n} 個系統匣圖示沒有回來，重新開啟那個應用程式就會出現。 | {n} tray icons didn't come back; reopening that app brings it back. |
| No XWayland | 這個桌面沒有 X11 相容層（XWayland），所以不會有舊式的 X11 系統匣圖示。 | This desktop has no X11 compatibility layer (XWayland), so there are no old X11 tray icons. |
| X11 tray selection held by another program | 另一個程式正在顯示舊式的 X11 系統匣圖示，所以這些圖示不會出現在頂端列。 | Another program is showing old X11 tray icons, so they won't appear in the top bar. |
| Bridge failed | 無法顯示舊式的 X11 系統匣圖示。 / 重試 | Old X11 tray icons can't be shown. / Try again |

The last three rows are shown only in their state (section 6.1); Try again
asks Better Manager to start the bridge's unit again through
`UserUnitController`, like any component unit.

Turning the component off in Apps stops the service and the bridge, closes the
view, gives GNOME's panel back, and restores every Better Defaults entry it
applied (the tray switch, the taskbar flags, the key bindings); removing it
does the same and deletes `$XDG_STATE_HOME/better-os/top-bar/` unless the
person unticks "Also delete my settings and history".

## 13. Copy that is new to the kit

| Key | zh-TW | English |
| --- | --- | --- |
| app_not_responding | 這個應用程式沒有回應。 | This app isn't responding. |
| desktop | 桌面 | Desktop |
| show_all_windows | 顯示所有視窗 | Show All Windows |
| app_menu_name | {app} 選單 | {app} menu |
| xembed_no_xwayland | 這個桌面沒有 X11 相容層（XWayland），所以不會有舊式的 X11 系統匣圖示。 | This desktop has no X11 compatibility layer (XWayland), so there are no old X11 tray icons. |
| xembed_selection_taken | 另一個程式正在顯示舊式的 X11 系統匣圖示，所以這些圖示不會出現在頂端列。 | Another program is showing old X11 tray icons, so they won't appear in the top bar. |
| xembed_failed | 無法顯示舊式的 X11 系統匣圖示。 | Old X11 tray icons can't be shown. |

## 14. Not verified, and owned by a ticket

| What | Owner |
| --- | --- |
| That a view started by `Meta.WaylandClient.spawnv` gets `WAYLAND_DISPLAY` as well as `WAYLAND_SOCKET` (GPUI picks Wayland by `WAYLAND_DISPLAY`, `gpui/src/platform.rs:103`) and that its first connection is the one that consumed `WAYLAND_SOCKET` (`wayland/client.rs:639` runs before the GPU probe at `:708`) | 151, first step, in a headless GNOME Shell |
| That `make_dock` on GPUI's window, `move_frame`, and a sized `xdg_toplevel` give a 32px strip at y = 0 that Mutter does not move or animate | 151 |
| That an `xdg_popup` grab from a click on a dock (which is not focused) gets keyboard input | 151 |
| That hiding `panelBox` drops its strut, and that GNOME's notification banners do not draw over the bar | 151, 158 |
| That every StatusNotifierItem on the machine re-registers when the watcher changes owner | 156 |
| That `gnome-session-x11-services.target` goes active when XWayland starts on demand and inactive when it stops, and that `DISPLAY` is in the user manager's environment by then | 149, first step |
| That a forwarded click reaches a GTK 2 and a GTK 3 XEmbed icon on XWayland under Mutter 46 while the pointer is over the bar, through X events and through XTest, and where each program's own menu opens | 149, first step |
| That logical bar coordinates convert to the right X root coordinates in Mutter's physical layout mode at scale 2 (the machine's) and in logical layout mode | 149, first step |
| That Mutter draws nothing for an override-redirect container at opacity 0 with an empty input region, and that `GetImage` on a redirected icon window returns its picture under XWayland | 149 |
| Which programs on the owner's machine dock an XEmbed icon, and that each comes back when the bridge takes the selection | 149, then 159 with the owner |
| The bar on the owner's desktop, at scale 2, with the person's own taskbar | 159 |
| The bar on COSMIC | 160, in the Better OS session (ADR 0038 option D, 2026-11-21) |
