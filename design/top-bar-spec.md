# Better OS top bar

A menu bar across the top of the screen, as on macOS: the Better OS menu and
the focused application's menus on the left, the tray, input method, network,
sound, battery, clock, and notifications on the right. This file says what the
bar contains, how each part behaves in every state, the exact copy in both
languages, how it is built on GNOME 46, and how it sits with the taskbar at the
bottom. The decision it needs is
[ADR 0036](/engineering/decisions/0036-top-bar-dock-surface.md) (proposed);
the work is cut into tickets 150 to 160. The look follows `DESIGN.md`, and
every control named here is a `better_ui::fluent` control. The mockups in
`design/top-bar/` draw the bar in both appearances, in zh-TW, on a 1440 × 900
logical screen (the owner's display at scale 2) with Zorin's taskbar as
decision D3 (a) leaves it: `rest-betteros` (a Better OS window focused, full
menus), `rest-other` (another application, shared actions only),
`betteros-menu`, `app-menu`, `tray-overflow`, `clock-flyout`, and `rest-960`
(a 960-wide screen), each as `-light.html` and `-dark.html`. The English copy is
in the tables below.

Where this file and a ticket differ, the ticket decides behaviour and this file
decides layout and copy.

## 0. Decisions that are the owner's

Nothing below is built until these are answered. Each has a recommendation and
what each answer costs.

| # | Decision | Recommendation | What each choice costs |
| --- | --- | --- | --- |
| D1 | **Which shape the bar is.** (A) A Better OS window drawn by Rust and GPUI, which GNOME Shell makes a *dock* and places at the top (section 9.3). (B) GNOME's own top panel, put back at the top of the screen, with a Better OS menu and an application menu that the extension draws in JavaScript (section 9.5). | **A.** | **A:** everything is Fluent and every decision is Rust with tests; the right side (tray host, network, sound, battery, input method, clock, notifications) is rebuilt in Rust, which is most of tickets 156–158. The extension grows a fourth kind of power: it starts one fixed program. **B:** the right side is GNOME's and works on the first day (about three tickets instead of ten), but the bar looks like Zorin's panel, not Fluent; the extension would hold the menu tree and draw labels it was sent, which breaks the "keeps nothing, takes no free text" bounds far more widely than A, and the mockups in `design/top-bar/` would not describe it. |
| D2 | **Accept ADR 0036**, which widens ADR 0018's bounds for the panel relay: start exactly one fixed program as a Wayland client and make its window a dock; reserve a strip of the screen; hide GNOME's own top panel while the bar runs; report the focused window and the shell's notifications; perform the window actions every window has. | **Accept.** | **Accept:** shape A can be built. **Refuse:** on GNOME 46 nothing but GNOME Shell can draw a panel (section 9.1), so the bar cannot exist without some JavaScript beyond today's bounds; the honest outcome is no top bar on GNOME, and a top bar only on COSMIC if ticket 143 goes that way (section 11). |
| D3 | **What happens to Zorin's bottom taskbar while the bar is on.** (a) It stays as it is, minus the things the bar now shows: its clock and its system menu (its tray icons already left with the tray switch, section 6.1). (b) It becomes a dock: centred icons, no clock, no system menu, no tray, hidden when a window touches it. (c) Ticket 142's Better OS taskbar replaces it. | **(a) now, (c) when ticket 142 ships.** | **(a):** one Better Defaults entry (two element flags in Zorin's own setting), undone exactly; Zorin's taskbar keeps its Start button and window list. **(b):** four more Zorin settings written (position of elements, intellihide, sizes), a bigger visible change, and still Zorin's code. **(c):** depends on ticket 142's own ADR and build; the two Better OS surfaces then share the dock relay this design adds, and Zorin's taskbar is turned off by the component (ticket 142's rule). Section 10 has the geometry for each. |
| D4 | **Ticket 142's taskbar left end**, which this bar affects because power, restart, lock, and log out move here: (a) the Better OS button beside Show Applications, Zorin's start button kept; (b) the Better OS button replaces Zorin's start button. | **(a) until ticket 94 makes Better Launcher a complete start menu, then (b).** | **(a):** two buttons that both open "everything" for a while. **(b):** Zorin's menu (with its own power items) goes away, so this bar's Better OS menu must be installed whenever (b) is: ticket 142's manifest declares the top bar as a dependency in that shape. |
| D5 | **A Better OS window's own menu row while the bar shows its menus.** (a) Hidden, as on macOS. (b) Kept as well. | **(a).** | **(a):** one place for menus; when the bar is off, missing, or has not accepted the window's menus within one second, the window shows its row again, so a window is never without menus. **(b):** the same commands twice on screen. |
| D6 | **Old X11 tray icons (XEmbed).** Zorin's AppIndicator extension also shows these. While the bar is on, that extension is off (section 6.1), and the bar shows StatusNotifierItems only. | **Accept: XEmbed icons are not shown while the bar is on.** | **Accept:** a program that only has an XEmbed icon has no icon while the bar is on; the bar's settings card says so. **Refuse:** keep Zorin's AppIndicator extension on and let both draw every StatusNotifierItem, which breaks the owner's "they show in the bar and not in the taskbar". |
| D7 | **Notifications in the bar.** (a) The clock opens a Better OS flyout that lists GNOME's notifications, relayed from the shell to the bar service only. (b) The clock opens GNOME's own calendar and notification list, which on Zorin lives in the bottom taskbar and opens from the bottom of the screen. | **(a).** | **(a):** a notification's app, title, and body cross the session bus to `better-top-bar` (the counterpart, checked by its executable path, as ADR 0018 does), ticket 151 carries the relay and ticket 158 the flyout. **(b):** nothing crosses; the calendar and the list stay at the bottom while the clock sits at the top. |

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
  ships `statusNotifierWatcher.js` and `trayIconsManager.js` (XEmbed), and has
  `legacy-tray-enabled true`, `tray-pos 'right'`; five items were registered.
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
  (`wayland-client-0.31.15/src/conn.rs:46-52`). Zed itself draws an in-window
  application menu on Linux (`crates/title_bar/src/application_menu.rs`).
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
- GTK `main`, `gtk/gtkapplication-wayland.c`
  (<https://raw.githubusercontent.com/GNOME/gtk/main/gtk/gtkapplication-wayland.c>):
  a GTK 4 `GtkApplication` window calls
  `gdk_wayland_toplevel_set_dbus_properties` with the application's menubar
  and app-menu object paths, which reach Mutter through `gtk_shell1`.
- KDE bug 424485 (<https://bugs.kde.org/show_bug.cgi?id=424485>): GTK's
  Wayland window-to-menu link is `gtk_shell1`, which KWin does not implement;
  GTK 4 dropped GtkModule loading, so `appmenu-gtk-module` cannot reach a
  GTK 4 application.
- Chromium `main`, `chrome/browser/ui/views/frame/dbus_appmenu_registrar.cc`
  and `ui/ozone/platform/wayland/host/wayland_toplevel_window.cc`
  (<https://raw.githubusercontent.com/chromium/chromium/main/chrome/browser/ui/views/frame/dbus_appmenu_registrar.cc>,
  <https://raw.githubusercontent.com/chromium/chromium/main/ui/ozone/platform/wayland/host/wayland_toplevel_window.cc>):
  on X11 it calls `com.canonical.AppMenu.Registrar.RegisterWindow`; on
  Wayland it announces the menu only through `org_kde_kwin_appmenu_manager`.
- Mozilla bug 1883184, "Initial support for GTK global menu"
  (<https://bugzilla.mozilla.org/show_bug.cgi?id=1883184>): landed in
  Firefox 125, disabled by default, exporting with libdbusmenu.
- Qt's Wayland platform plugin wraps `org_kde_kwin_appmenu_manager`
  (<https://codebrowser.dev/qt6/qtbase/src/plugins/platforms/wayland/qwaylandappmenu_p.h.html>);
  Qt's `QGenericUnixTheme` exports a `QMenuBar` and registers it with
  `com.canonical.AppMenu.Registrar` (KDE review
  <https://phabricator.kde.org/D3085?id=9146>).
- libcosmic's panel applet chapter
  (<https://pop-os.github.io/libcosmic-book/panel-applets.html>) and the Arch
  wiki's COSMIC page (<https://wiki.archlinux.org/title/Cosmic>).

## 1. The bar

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│ ◆  Better Files  檔案  編輯  檢視  前往  視窗  說明        ▴ ◈ ◈ ◈  注  ᯤ  🔊  🔋 87%  11月21日 週六 下午3:42  🔔 │ 32
└─────────────────────────────────────────────────────────────────────────────────────────┘
 ▲  ▲             ▲                                    ▲          ▲   ▲   ▲    ▲         ▲               ▲
 │  app name      app menus                            overflow   tray    IME net  sound battery  clock      notifications
 Better OS menu
```

- **Height** 32 logical pixels at every scale (a standard control height in
  `DESIGN.md`); 64 physical pixels at scale 2. Full width of the monitor it is
  on.
- **Fill**: the window background (`#f3f3f3` light, `#202020` dark) with a 1px
  bottom edge in the card stroke. No shadow, no radius: it meets the screen's
  edges. The bar follows the person's appearance like every Better OS window
  (ADR 0019).
- **Items**: each item is a button 32px tall, 8px side padding (12px for menu
  titles), with the subtle fill on hover and while its menu is open, 4px
  radius inset 2px from the bar's top and bottom. Text is Body (14/20); the
  application name is Body Strong. Icons are 16px. Tray and status icons are
  single-colour symbolic icons (`DESIGN.md`, Icons), the Better OS mark is the
  one coloured picture.
- **Order**, leading to trailing: Better OS menu, application name, application
  menus, then a flexible gap, then tray, input method, network, sound,
  battery, clock, notifications. Nothing is centred.
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
in the person's language). Truncated with an ellipsis at 200px.

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
`<Alt>F4` on the machine); the bar never invents one. For a Better OS window
these items are not drawn here: its own Window and File menus carry them
(section 5.2), and the application name opens a menu of About {app}, Settings…
and Quit {app}, as on macOS.

Accessible name: {app} 的應用程式選單 / {app} application menu.

## 5. Application menus

### 5.1 Where a window's menus come from

The bar asks, in this order, and uses the first that answers for the focused
window:

| Source | Which programs | How the bar finds the menu | Protocol |
| --- | --- | --- | --- |
| Better OS registration | every Better OS window (section 5.2) | the window's process registers its menu with `org.betteros.TopBar1.Menus.Register(o path)`; the service matches the caller's PID (`GetConnectionUnixProcessID`) to the focused window's PID the shell reports, and the window's own `app_id` | `com.canonical.dbusmenu` |
| GTK's window link | GTK 3 and GTK 4 `GtkApplication` windows that set a menubar, and GTK 3 windows with `appmenu-gtk-module` loaded | Mutter's `get_gtk_unique_bus_name` and `get_gtk_menubar_object_path`, reported by the shell | `org.gtk.Menus` and `org.gtk.Actions` |
| AppMenu registrar | X11 and XWayland programs: Qt with its platform theme, Chromium and Electron on X11, GTK 3 with `appmenu-gtk-module` | the bar service owns `com.canonical.AppMenu.Registrar` and looks up the window's X11 id, which the shell reports for an X11 window | `com.canonical.dbusmenu` |
| none of these | everything else | — | the actions every window has (section 4) only |

The registrar is taken only when no other program owns
`com.canonical.AppMenu.Registrar`; when one does, the bar uses it as a client
(`GetMenuForWindow`) instead and says so in its settings card. When the bar is
turned off, the name is released.

What exports a menu today, so the person knows what to expect:

| Toolkit | Exports a menu the bar can show on GNOME 46 Wayland | Evidence |
| --- | --- | --- |
| Better OS (GPUI) | Yes, after ticket 154 | GPUI has none (`gpui_linux/.../platform.rs:616-628`); `better-ui` adds it |
| GTK 4 / libadwaita | Only a `GtkApplication` that sets a menubar; libadwaita applications normally have none (they use a header-bar menu) | `gtkapplication-wayland.c` |
| GTK 3 | A `GtkApplication` with a menubar, natively; any `GtkMenuBar` only with `appmenu-gtk-module`, which is not installed on the machine | `gtkapplication-wayland.c` (same path in GTK 3), KDE bug 424485 |
| Qt 5 / 6 | Only under XWayland (registrar); on Wayland Qt announces it through `org_kde_kwin_appmenu`, which Mutter does not offer | Qt's `qwaylandappmenu`, the registry list above |
| Chromium, Electron | Only under XWayland (registrar); on Wayland only `org_kde_kwin_appmenu` | Chromium's `dbus_appmenu_registrar.cc`, `wayland_toplevel_window.cc` |
| Firefox | Off by default since 125; when enabled it uses libdbusmenu and a Wayland annotation protocol Mutter 46 does not offer | Mozilla bug 1883184 |

The ticket's premise that "GTK 4 applications have no menu bar to export" is
partly wrong: GTK 4 does link a `GtkApplication` menubar to its window through
`gtk_shell1`, and GNOME Shell can read it. Few GNOME applications set one.

### 5.2 Better OS's own applications

Every Better OS window that has menus exports all of them, and the bar shows
them after the application name. Each window's menus are built from one model
in `better-ui` (ticket 154), which is also what draws its in-window menu row
when the bar is not there. The rule for what is in them: **every command the
window offers in its command bar, its context menus, or its key map is in
exactly one menu, with its shortcut**, and a test asserts it per window. The
standard menus, in this order, with their fixed items:

| Menu | zh-TW | English | Fixed items (zh-TW / English) |
| --- | --- | --- | --- |
| Application (the name) | {app} | {app} | 關於 {app} / About {app} · 設定… / Settings… (only if the window has settings) · 結束 {app} / Quit {app} (Ctrl+Q) |
| File | 檔案 | File | 開新視窗 / New Window (Ctrl+N, where the window supports more than one) · 關閉視窗 / Close Window (Ctrl+W) |
| Edit | 編輯 | Edit | 復原 / Undo (Ctrl+Z) · 重做 / Redo (Ctrl+Shift+Z) · 剪下 / Cut (Ctrl+X) · 複製 / Copy (Ctrl+C) · 貼上 / Paste (Ctrl+V) · 全選 / Select All (Ctrl+A) · 尋找 / Find (Ctrl+F) — each disabled when the focused field or view cannot do it |
| View | 檢視 | View | the window's own view commands |
| (the window's own menus) | | | Better Git's 儲存庫 / Repository and 分支 / Branch; Better Files' 前往 / Go |
| Window | 視窗 | Window | 最小化 / Minimize · 全螢幕 / Enter Full Screen · 關閉視窗 / Close Window · then one item per open window of the application, the focused one checked |
| Help | 說明 | Help | 鍵盤快速鍵 / Keyboard Shortcuts · 回報問題 / Report a Problem (opens the project's issue page) · 關於 {app} / About {app} |

Per window, in tickets 154 and 155:

| Window | Menus |
| --- | --- |
| Better Git | today's four (File, View, Repository, Branch) with their items unchanged (`render.rs:860-960`), plus Edit, Window, Help |
| Better Files | File, Edit, View, Go, Window, Help |
| Better Monitor | File (Export…), Edit, View (its views and grouping), Window, Help |
| Better Manager | File, Edit, View (one item per page), Window, Help |
| Better Launcher, the app chooser | none: they are panels and dialogs, not applications, and the bar keeps showing the window behind them |

A menu item's state (enabled, checked) is the window's own and changes while
the menu is open, through dbusmenu's `ItemsPropertiesUpdated`. Activating an
item sends dbusmenu's `Event("clicked")`, which the window turns into the same
action its in-window menu or key performs.

### 5.3 Drawing a menu

- Titles in Body, 12px side padding. A title the window marks disabled is in
  disabled text and opens nothing.
- Items are `fluent::menu` rows: an icon slot (16px) only when any item in the
  menu has an icon or a check, the label, the shortcut in secondary text at the
  trailing edge, a chevron for a submenu. Separators are 1px card stroke with
  4px above and below. A check item shows a check glyph; a radio item a dot.
- A label longer than 360px is truncated with an ellipsis; its full text is the
  tooltip and the accessible name.
- An item a toolkit marks hidden is not drawn. A submenu that is empty is drawn
  disabled.
- Mnemonics (`_File`, `&File`) are not shown as underlines; the character is
  used for type-to-select inside an open menu.
- A menu whose exporter does not answer within 500 ms opens with one disabled
  row: 這個應用程式沒有回應。 / This app isn't responding. The bar never waits
  longer than that before opening something.

### 5.4 When there are more menus than room

Menus that do not fit between the application name and the right side are
moved, from the last, into a 更多 / More (`»`) menu whose submenus are the
moved menus. The right side is never pushed off the bar: section 8 says what
shrinks first.

## 6. The right side

### 6.1 The tray (StatusNotifierItem)

While the bar is on, every StatusNotifierItem shows here and not in the bottom
taskbar (owner, 2026-11-12). The bar service is the `StatusNotifierHost`, and
the `StatusNotifierWatcher` while it is on, because the watcher on the machine
today is Zorin's AppIndicator extension, and that extension must be off for
its icons to leave the taskbar.

**The switch is one Better Defaults entry**, "Tray icons in the top bar",
applied when the bar is turned on and restored when it is turned off or
removed, with Better Defaults' capture, apply, verify, and restore:

| Desktop | Applied | Restored |
| --- | --- | --- |
| Zorin OS (`zorin-appindicator@zorinos.com`) | the extension is taken out of `org.gnome.shell enabled-extensions`, its previous presence captured | put back exactly where it was in the list, if it was there |
| Ubuntu (`ubuntu-appindicators@ubuntu.com`) | the same | the same |
| Neither present | nothing to change; the entry reports "nothing to do" | — |

The order matters and is the service's: on turning on, it takes
`org.kde.StatusNotifierWatcher` the moment the extension releases it; on
turning off, it releases the name before the extension is put back. Items that
follow the specification watch the watcher's name and register again with the
new owner (Better OS's own trays do since tickets 97 and 102; that libappindicator and the other toolkits' items do is not verified here, and ticket 156 checks it).
An item that does not re-register is missing until it restarts; the bar's
settings card lists how many items were registered before the switch and how
many came back, so a missing one is visible: 有 {n} 個系統匣圖示沒有回來，重新開啟那個應用程式就會出現。 /
{n} tray icons didn't come back; reopening that app brings it back.

Icons: `IconName` from the icon theme, or `IconPixmap` scaled to 16 logical
pixels (32 at scale 2, picking the nearest size the item offers), with the
item's `Status`: `Passive` items go to the overflow, `NeedsAttention` shows
`AttentionIconName` or the pixmap and a dot. Clicking: primary button calls
`Activate(x, y)` when the item has no menu or `ItemIsMenu` is false, else
opens its `com.canonical.dbusmenu` menu, drawn as section 5.3; secondary button
opens the menu; middle button `SecondaryActivate`; scroll `Scroll`. Tooltip:
`ToolTip`'s title, else `Title`. Accessible name: the same text, then
系統匣 / tray.

**Overflow.** At most six items are shown on the bar (four below 1280px wide);
the rest, and every `Passive` item, sit behind a `▴` chevron that opens a
flyout: a grid of 48 × 40px cells, six per row, each the item's icon with its
title under it in Caption, truncated to the cell. Which items stay on the bar: the ones the person dragged
there (order is kept by item id in the service's own state,
`$XDG_STATE_HOME/better-os/top-bar/tray.json`), then registration order.

| | zh-TW | English |
| --- | --- | --- |
| Chevron accessible name | 顯示隱藏的圖示 | Show hidden icons |
| Overflow flyout title | 系統匣 | Tray |
| Overflow hint (Caption) | 拖曳圖示到頂端列就會固定顯示。 | Drag an icon to the top bar to keep it there. |
| Overflow empty | 沒有其他圖示。 | No other icons. |
| No items at all | the chevron and the area are not drawn | — |
| XEmbed icons (D6) | the bar's settings card | 舊式的 X11 系統匣圖示不會顯示在頂端列。 | Old X11 tray icons aren't shown in the top bar. |

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

Shown only when UPower's display device is a battery. Icon by charge and state,
with the percentage after it in Body (`87%`). Flyout:

| Element | zh-TW | English |
| --- | --- | --- |
| Line 1 | {n}% · {state} | {n}% · {state} |
| States | 充電中 / 使用電池 / 已充滿 / 已接電源，沒有充電 | Charging / On battery / Full / Plugged in, not charging |
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

Accessible names: 通知，{n} 則未讀 / Notifications, {n} unread; 日期與時間：{date} /
Date and time: {date}.

## 7. Keyboard access and screen readers

| Key | What it does |
| --- | --- |
| Ctrl+F2 | focuses the bar: the Better OS menu is highlighted (macOS's "move focus to the menu bar") |
| Left, Right | move between items; with a menu open, open the neighbouring menu |
| Down, Enter, Space | open the highlighted item's menu or flyout; in a menu, Down and Up move, Right opens a submenu, Left closes it |
| type a letter | in an open menu, jump to the next item starting with it |
| Escape | close the menu; a second Escape returns focus to the window that had it |
| Super+V, Super+M | open the notifications flyout |

Super+V and Super+M are GNOME's own keys for its calendar and notification
list (`org.gnome.shell.keybindings toggle-message-tray ['<Super>v',
'<Super>m']` on the machine). While the bar is on, Better Defaults moves them:
GNOME's `toggle-message-tray` is set to `[]` and two GNOME custom key bindings
take the same keys, and Ctrl+F2 (unbound on the machine) is a third. Turning
the bar off restores `toggle-message-tray` and removes the custom bindings,
with the changed-elsewhere rule ticket 94 uses. The custom bindings run
`better-top-bar --focus` and `better-top-bar --notifications`, which ask the
running service over its bus name. `<Super>n` stays GNOME's
`focus-active-notification`, which acts on GNOME's banner. A dock does not take focus by itself; the shell focuses the bar's window
when the service asks (the panel relay's `FocusView`), and gives focus back to
the window that had it when the menu closes.

Every item, menu, and row has an accessible name (the tables above) and a role
(menu bar, menu item, check menu item, button, toggle button), exposed through
GPUI's AccessKit adapter. Status changes the person should hear (battery low
at 10%, a network lost) are announced politely, never more than once per
change.

Focus is drawn with `DESIGN.md`'s focus visual, only when it came from the
keyboard.

## 8. Sizes, scale, monitors, direction

**Width.** As the bar narrows, things give way in this order, so the Better
OS menu, the application name, and the clock's time are always visible:

1. Below 1280 logical px: the clock drops the date and weekday; at most four
   tray items.
2. Then application menus move into 更多 / More from the last (section 5.4).
3. Below 1024: the battery drops its percentage text; tray items all go into
   the overflow.
4. Below 960: the application name truncates to 120px. (The machine's own
   display is 1440 logical px wide at scale 2; 960 is the narrowest width
   Better OS windows are designed for, and a display set to a larger scale or a
   small external monitor reaches it.)

The mockup `rest-960-*.html` draws the 960px bar.

**Scale.** The bar is laid out in logical pixels; at scale 2 everything is
twice as many physical pixels and drawn sharp (GPUI renders at the output's
scale). Fractional scale through `wp_fractional_scale_manager_v1`, which the
compositor offers. Mockups are drawn at scale 1.

**Monitors.** The bar is on the primary monitor (Mutter's primary, the one
with GNOME's top panel). A setting, 在所有螢幕顯示頂端列 / Show the top bar on
every display, off by default, draws one bar per monitor; each shows the menus
of the focused window, as macOS does. A monitor connected or removed, or the
primary changed, moves or adds the bar within a second (the shell reports
`monitors-changed`). A monitor with its own scale gets a bar drawn at that
scale.

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
  and which Better OS must not change. It was not tried on the machine.

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
application, PID, GTK menu paths, and X11 id. Zorin's own desktop icons
extension uses the same route for its desktop window
(`zorin-desktop-icons@zorinos.com/extension.js:511-525`,
`emulateX11WindowType.js:216-232`). On GNOME 46 a client can become a dock
only if the shell started it: `new_indirect` and `setup_fd`, which would let an
outside process connect as such a client, are not callable from GJS.

### 9.3 The shape: a Better OS window the shell makes a dock (D1 A)

Two Better OS processes, and the relay inside the shell:

```
 better-top-bar.service (user unit, Rust)          the shell (GJS relay)            better-top-bar-view (GPUI)
 ───────────────────────────────────────          ─────────────────────            ──────────────────────────
 owns org.betteros.TopBar1                 ──► StartView() ──► spawnv(fixed path) ──► connects through
 decides everything: layout, menus,                make_dock, move_frame,              WAYLAND_SOCKET, draws
 tray host + watcher, registrar,           ◄── FocusChanged, Monitors, Fullscreen    the bar and its menus
 network, sound, battery, clock,           ──► ReserveStrip(monitor, edge, px)        (xdg_popup), talks only
 notifications list                        ◄── Notification*, InputSources            to the service
                                           ──► WindowAction(id, action)
```

- **`better-top-bar`** (crate `top-bar-service`, a systemd user unit in the
  `better-top-bar` package, enabled by installing like every service). It owns
  `org.betteros.TopBar1`, is the StatusNotifierHost and Watcher (section 6.1),
  the AppMenu registrar (5.1), reads NetworkManager, UPower, the audio server,
  and the clock settings, holds the notification list, and decides every state
  in this file in `top-bar-core`, a crate with no GPUI dependency and with the
  tests.
- **`better-top-bar-view`** (crate `top-bar-gui`), at
  `/usr/libexec/better-os/top-bar-view`, started by the shell and by nothing
  else. It draws what the service tells it, as a GPUI window on the shell's
  Wayland client connection, and menus and flyouts as `AnchoredPopup`
  (`xdg_popup`) children of it. It mounts `UpdateRestart`, but restarting in
  place would lose the shell's connection, so when its program is replaced it
  exits instead and the service asks the shell for a new one (AGENTS.md's
  restart rule is met: the new program runs within seconds).
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
| `FocusChanged(t window, s app_id, s app_name, u pid, u client_type, t xwindow, s gtk_bus_name, s gtk_menubar_path, u flags)` signal | | The focused window as the shell sees it; `flags` bits: can minimize, can fullscreen, is fullscreen, can close, can open new window, belongs to the view's client (`owns_window`); whether a window is a Better OS panel is decided in Rust from its `app_id`. Empty strings and 0 where a value does not exist. |
| `Monitors(a(uiiiidb))` signal | | Index, logical x, y, width, height, scale, is primary, for every monitor, on `monitors-changed`. |
| `FullscreenChanged(u monitor, b)` signal | | |
| `InputSources(a(uss) sources, u current)` signal, `ActivateInputSource(u index)` | | GNOME's input sources as the shell's `InputSourceManager` has them (index, id, short name); the engine's properties as GNOME's keyboard indicator holds them with a serial number; `ActivateInputProperty(u serial, u index)` resolves the index against the shell's current list at call time and is refused when the serial is stale. |
| `Notification(t id, s app, s title, s body, x time, u urgency, as action_labels)` signal, `NotificationRemoved(t id)`, `ListNotifications()`, `ActivateNotification(t id)`, `NotificationAction(t id, u index)`, `DismissNotification(t id)`, `ClearNotifications()` | | GNOME's message tray, copied unchanged, sent to the counterpart only. |
| `Capabilities()` | out `s` | JSON: protocol version, shell version, whether the view runs, the strips held, GNOME's panel found and hidden. |

Every method checks the caller against the counterpart's unique name and the
counterpart's executable, as ADR 0018 does; every signal goes to the
counterpart only.

### 9.5 Shape B, for comparison (D1 B)

Zorin's taskbar puts GNOME's panel back at the top when its
`stockgs-keep-top-panel` is true (`panelManager.js:67`), and then every
AppIndicator icon, the keyboard indicator, quick settings, and the date menu
are in that top panel with no Better OS code. Better Defaults would write that
one setting, plus `panel-element-positions` to hide the bottom panel's own
copies. What is missing is the left side: GNOME 46 has no application menu in
its panel, so the extension would add a Better OS menu button and an
application menu button and draw their menus (`PopupMenu`) from the dbusmenu
trees the same Rust service exports, keeping the tree and the labels in
JavaScript. It is fewer tickets and less risk on the right side, and it is not
Fluent; ADR 0036 records why it was not chosen.

### 9.6 What GPUI and `better-ui` need for exporting menus

GPUI has no Linux menu export (sources above), and Better OS's windows do not
use GPUI's `set_menus` at all: Better Git draws its own menu row. So the work
is in `better-ui`, not in GPUI:

- **A menu model** in `better-ui`'s GPUI-free part: menus, items (label,
  shortcut, enabled, checked, radio group, submenu, separator), and an action
  id per item, built by each window from its own state. The in-window menu row
  (`fluent::menu`) draws from it, so the two can never differ.
- **A `com.canonical.dbusmenu` exporter** for that model: `GetLayout`,
  `GetGroupProperties`, `Event`, `AboutToShow`, `LayoutUpdated`,
  `ItemsPropertiesUpdated`. The exporter code that `awake-tray` and
  `monitor-tray` each have today moves into one crate, `dbusmenu`, which the
  trays, the windows, and the bar's host all use (AGENTS.md: one rule, one
  place).
- **Registration** with `org.betteros.TopBar1.Menus.Register(o)` from the
  window's own session-bus connection, once per process, with the menu of
  whichever of its windows is active; and the window hides its own row while
  the bar has accepted the registration (D5).
- **Nothing in GPUI.** Exporting through GTK's `gtk_shell1` link would need
  GPUI's Wayland connection to bind `gtk_shell1` and call
  `gtk_surface1.set_dbus_properties`, and the bar would then have to speak
  `org.gtk.Menus` to Better OS windows. A Better OS registration keyed by PID is
  enough because the bar and the windows are both ours; if a GPUI change is ever
  wanted (for other desktops to see Better OS menus), it is `gtk_shell1` support
  upstream in `gpui_linux`, not something this design needs.

## 10. The taskbar at the bottom

Today (owner's machine): Zorin's taskbar at the bottom, 45px, holding Zorin's
start menu, Show Applications, the window list, the tray, the system menu
(network, sound, battery, power), the clock, and the desktop button.

| | Zorin taskbar kept (D3 a) | Zorin taskbar as a dock (D3 b) | Better OS taskbar (D3 c, ticket 142) |
| --- | --- | --- | --- |
| Top edge | the bar, 32px reserved | the bar | the bar |
| Bottom edge | Zorin's taskbar, 45px reserved | Zorin's taskbar, centred, intellihide on, nothing reserved | Better OS's taskbar, reserved by the same `ReserveStrip(monitor, 1, px)` |
| Clock, system menu, tray at the bottom | hidden through Zorin's `panel-element-positions` (`dateMenu`, `systemMenu` `visible: false`); `rightBox` stays, because it holds shell indicators the bar cannot draw (drive menu, printers, Zorin Connect), minus GNOME's keyboard indicator, which the relay hides while the bar shows the input method | hidden the same way | not drawn: the Better OS taskbar has none while the bar is on |
| Start / Better OS button | Zorin's | Zorin's | ticket 142's (D4) |
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

Recommendation: **(a) now, (c) when ticket 142 ships.** (a) is the smallest
visible change that meets the owner's tray rule and leaves the person's
taskbar recognisable; (b) changes how a person switches windows for no gain
the bar brings; (c) is the long-term answer and is already decided in
direction.

## 11. On COSMIC

If ticket 143 makes COSMIC the desktop, the bar becomes simpler: cosmic-comp
offers layer-shell, on which COSMIC's own panel and its applets are built
(libcosmic's panel applet chapter), so `better-top-bar-view` would open its
window as GPUI's `WindowKind::LayerShell` anchored to the top edge with an
exclusive zone, and the panel relay is not needed. The tray host, network,
sound, battery, clock, and menus are the same Rust, since none of them depends
on GNOME; the input-method and notification parts would need COSMIC's own
sources instead of GNOME Shell's, which ticket 143's evaluation must name.
Whether COSMIC's panel is turned off or kept is that ticket's decision.

## 12. Better Manager: the Top bar card

The bar is a component, `better-top-bar`, installed, updated, turned off, and
removed through Better Manager. Its settings live on a control center card
under Settings › 頂端列 / Top bar:

| | zh-TW | English |
| --- | --- | --- |
| Header | 頂端列 | Top bar |
| Description | 螢幕上方的選單列：Better OS 選單、應用程式選單、系統匣、時鐘和通知。 | The menu bar at the top of the screen: the Better OS menu, app menus, tray, clock, and notifications. |
| Every display | 在所有螢幕顯示頂端列 | Show the top bar on every display |
| Tray note (Caption) | 頂端列開啟時，系統匣圖示會從工作列移到這裡。 | While the top bar is on, tray icons move here from the taskbar. |
| XEmbed note | 舊式的 X11 系統匣圖示不會顯示在頂端列。 | Old X11 tray icons aren't shown in the top bar. |
| Registrar held by another program | 另一個程式正在提供應用程式選單，頂端列會使用它。 | Another program provides app menus; the top bar uses it. |

Turning the component off in Apps stops the service, closes the view, gives
GNOME's panel back, and restores every Better Defaults entry it applied (the
tray switch, the taskbar flags, the key bindings); removing it does the
same and deletes `$XDG_STATE_HOME/better-os/top-bar/` unless the person unticks
"Also delete my settings and history".

## 13. Copy that is new to the kit

| Key | zh-TW | English |
| --- | --- | --- |
| more_menus | 更多 | More |
| app_not_responding | 這個應用程式沒有回應。 | This app isn't responding. |
| desktop | 桌面 | Desktop |
| show_all_windows | 顯示所有視窗 | Show All Windows |

## 14. Not verified, and owned by a ticket

| What | Owner |
| --- | --- |
| That a view started by `Meta.WaylandClient.spawnv` gets `WAYLAND_DISPLAY` as well as `WAYLAND_SOCKET` (GPUI picks Wayland by `WAYLAND_DISPLAY`, `gpui/src/platform.rs:103`) and that its first connection is the one that consumed `WAYLAND_SOCKET` (`wayland/client.rs:639` runs before the GPU probe at `:708`) | 151, first step, in a headless GNOME Shell |
| That `make_dock` on GPUI's window, `move_frame`, and a sized `xdg_toplevel` give a 32px strip at y = 0 that Mutter does not move or animate | 151 |
| That an `xdg_popup` grab from a click on a dock (which is not focused) gets keyboard input | 151 |
| That hiding `panelBox` drops its strut, and that GNOME's notification banners do not draw over the bar | 151, 158 |
| That every StatusNotifierItem on the machine re-registers when the watcher changes owner | 156 |
| The bar on the owner's desktop, at scale 2, with the person's own taskbar | 159 |
| The bar on COSMIC | 160, after ticket 143's decision |
