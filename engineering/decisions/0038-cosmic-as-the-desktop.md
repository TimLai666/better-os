# ADR 0038: COSMIC as Better OS's desktop — stay on GNOME, support both, or move

## Status

Proposed on 2026-11-21 for ticket 143, for the owner to decide. Nothing it
describes is built until the owner chooses an option; the tickets it names are
written after that choice.

## The owner's decisions

| # | Decision | Recommendation |
| --- | --- | --- |
| C1 | Which option: **A** stay on GNOME, **B** support both with GNOME as the default, **C** make COSMIC the primary desktop | **B**, staged as below: the cheap desktop-neutral pieces first, the one-click COSMIC component last, after a pinned install has been proved on Zorin OS 18 in a VM |
| C2 | Whether Better Manager may add System76's APT source, its signing key, and an APT pin to a person's machine (AGENTS.md: no public APT repository or signing implementation without a decision) | **Yes, narrowly**: one source file, one keyring holding the one fingerprint below, and a pin that takes only the 26 COSMIC package names from it |
| C3 | Login screen when COSMIC is installed: keep GDM, or let `cosmic-greeter` take over | **Keep GDM**; it lists the COSMIC session, and ADR 0033's watch keeps working |
| C4 | Input method on COSMIC: install Fcitx 5 with `fcitx5-chewing` in place of IBus | **Yes**; IBus 1.5.29 offers COSMIC no Wayland input method, Fcitx 5 was seen typing Zhuyin into a Better OS window |
| C5 | Better OS windows' frame on COSMIC: the GTK frame they draw today, or COSMIC's own title bar | **COSMIC's own title bar** (server-side), so they match every COSMIC window |
| C6 | Who draws the desktop's icons (ticket 144) | **Better Files**: a layer-shell surface on COSMIC, and on GNOME a second row in ADR 0036's program table that replaces Zorin's extension while it runs |
| C7 | COSMIC's own panel while Better OS's top bar runs | **Hidden, and put back exactly** when the bar is turned off or removed |

## Context

On 2026-11-16 the owner asked whether Better OS should move to COSMIC,
System76's Rust desktop, and set the bar for any move: once Better OS is
installed, one action in Better Manager installs COSMIC and switches the person
to it, with nothing else to set up, and removing it puts the previous desktop
back. Better OS today is written against GNOME 46 on Zorin OS 18 and Ubuntu.

Every claim below names its source: a URL with the date it was read, a file in
an upstream repository at the commit read, a command run on this machine
(read-only), or an observation in the VM described under "What was run". All
web reads were on 2026-11-21. "Not run" marks what was not observed.

### COSMIC today

- **Releases.** Epoch 1.0.0 was tagged on 2025-12-10 (`pop-os/cosmic-epoch` tag
  `epoch-1.0.0`, commit `39c24b0`). Point releases followed every one to three
  weeks: 1.0.1 (2025-12-30) to 1.0.16 (2026-06-10), then 1.1.0 (2026-06-23),
  1.2.0 (06-30), 1.3.0 (07-14), 1.4.0 (07-22), 1.5.0 (07-29), 1.6.0 (08-18),
  1.7.0 (08-25), 1.8.0 (09-10), 1.9.0 (09-23), 1.10.0 (10-08), and 1.11.0
  (2026-10-20), the newest (<https://github.com/pop-os/cosmic-epoch/releases>).
  Each release changes behaviour (1.11.0 implemented `xdg-dialog-v1` and fixed
  input methods using the virtual keyboard protocol; 1.10.0 moved GNOME font
  syncing into `cosmic-settings-daemon`), so it is a moving target.
- **The image tested.** Pop!_OS 24.04 build 29, the ISO System76's build API
  names (<https://api.pop-os.org/builds/24.04/generic>), carries
  `cosmic-session 1.11.0~1794011892`, `cosmic-comp 0.1~1794504377~f2beba6`,
  `cosmic-files 1.11.0`, `xdg-desktop-portal-cosmic 1.11.0`, `greetd 0.10.0`,
  `libwayland-server0 1.26.0-1pop1`, `pipewire 1.6.8-1pop1`, Mesa 26.1.6, and
  `ibus 1.5.29-2`, and no `gdm3` and no Tracker
  (`pop-24.04-build29-filesystem.manifest` in the evidence folder).
- **Stable and not.** Everything the session starts is in the stable release:
  the compositor, panel and dock, the app library, launcher, workspaces view,
  notifications, idle and lock, the greeter, the portal, and COSMIC Files'
  desktop mode (below). What is missing or fixed in code:
  - **Touchpad gestures are not configurable.** `cosmic-comp`'s
    `src/input/mod.rs` (master `6968c5d`, 2026-11-20, lines 1195–1300) takes
    every swipe of three or more fingers for itself whenever the workspace
    overview is closed: three fingers do nothing ("TODO: 3 finger gestures"),
    four fingers switch workspaces in the layout's direction, and neither
    reaches an application. Pinches and holds are forwarded to the focused
    client. There is no setting and no protocol that hands a gesture to an
    outside process.
  - **Global shortcuts** are COSMIC's own configuration, not a portal: the
    portal implements `Access`, `FileChooser`, `RemoteDesktop`, `Screenshot`,
    `Settings`, and `ScreenCast` only (`data/cosmic.portal` on master, and the
    installed file in the VM). Super alone is `System(Launcher)` and
    Super+Space is `System(InputSourceSwitch)`
    (`/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults`,
    lines 102 and 108, in the VM), and `system_actions` maps `Launcher` to the
    command `cosmic-launcher`, which a per-person file under
    `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/` overrides.
  - **Fractional scaling** works: `wp_fractional_scale_manager_v1` is
    advertised, and Better OS windows were sharp at 100 %, 150 %, and 200 %.
  - **Input methods:** `cosmic-comp` offers `zwp_input_method_manager_v2`,
    `zwp_text_input_manager_v3`, and `zwp_virtual_keyboard_manager_v1`. No
    input method runs by default in the Pop image (`pgrep ibus` and
    `pgrep fcitx` empty). Fcitx 5 with `fcitx5-chewing`, installed in the VM,
    typed 你 in Zhuyin (s, u, 3) into Better Launcher's search field, with an
    underlined composition and a commit on Enter. IBus is not used: Pop's
    image has no `ibus-wayland`, and IBus 1.5.29's Wayland front end speaks
    `zwp_input_method_v1` (from IBus's source, medium confidence; not run),
    which `cosmic-comp` does not offer. A GTK application can still reach
    IBus through its GTK module, but GPUI has no such module, so on COSMIC a
    Better OS window gets Zhuyin only through Fcitx 5.
  - **Tray:** `cosmic-applet-status-area --status-notifier-watcher` owns
    `org.kde.StatusNotifierWatcher` (seen on the session bus). Better Awake's
    and Better Monitor's tray icons appeared in the panel and Better Awake's
    menu opened from its icon (screenshots 05, 06).
  - **Screen locking and login:** `cosmic-greeter` is both the login screen
    (under `greetd`) and the lock screen (`ext_session_lock_manager_v1`);
    `loginctl lock-session` locks. `cosmic-idle` blanks the screen and suspends
    on its own timers and honours only `org.freedesktop.ScreenSaver.Inhibit`
    (`pop-os/cosmic-idle` `src/freedesktop_screensaver.rs`, `src/main.rs`
    lines 192–228).
  - **Accessibility:** `cosmic_a11y_manager_v1` (magnifier and screen
    filters), `org.freedesktop.a11y.Manager` on the bus, and Orca 49.4 in the
    image. How well COSMIC's own applications speak AT-SPI was not assessed.
  - **XWayland** runs rootless (`Xwayland :1 -rootless`, observed), with a
    `descale_xwayland` option.
  - **Decorations and corners:** `cosmic-comp` offers `zxdg_decoration_manager_v1`
    and KDE's server-decoration protocol. Its default preference is
    server-side (`cosmic-comp-config/src/lib.rs`, `DecorationPreference`,
    default `ServerSide` since commit `63dacd14`, 2026-11-06), but a client
    that asks for its own frame gets it (`src/wayland/handlers/decoration.rs`,
    `request_mode`); a window in a tab stack is always server-side. The
    compositor clips every floating and tiled window to the theme's small
    radius (`AppearanceConfig { clip_floating_windows: true,
    clip_tiled_windows: true }`, added 2026-01-13 in commits `2adebb5f` and
    `ea470f0a`; `src/shell/element/window.rs`, `corner_radius`), except when
    maximized. Better Files' bottom corners, which Better OS draws square on
    GNOME (ticket 141), came out rounded (screenshots 02a, 02b).
  - **Protocols open to an outside client.** From `src/state.rs` (lines
    677–776) and `wayland-info` in the VM (`wayland-info-and-portals.txt`):
    the core and stable set (`wl_compositor` 5, `xdg_wm_base` 7, viewporter,
    presentation, cursor-shape 2, fractional scale, `xdg_activation_v1`,
    `xdg_wm_dialog_v1`, `zxdg_exporter_v2`/`importer`, pointer constraints,
    relative pointer, pointer gestures 3, tablet 2, primary selection,
    `wp_alpha_modifier_v1`, single-pixel buffer, `wl_fixes`,
    `wp_security_context_manager_v1`, `ext_background_effect_manager_v1`,
    keyboard-shortcuts inhibit, `zwp_idle_inhibit_manager_v1`,
    `ext_idle_notifier_v1` 2), and, for a client not inside a security
    context (`client_not_sandboxed`, so any ordinary process and not a
    Flatpak): `zwlr_layer_shell_v1` 5, `ext_foreign_toplevel_list_v1`,
    `zcosmic_toplevel_info_v1` 3, `zcosmic_toplevel_manager_v1` 4 (close,
    activate, maximize, minimize, move to workspace, sticky),
    `ext_workspace_manager_v1` and `zcosmic_workspace_manager_v2`,
    `zwlr_output_manager_v1` 4 and `zcosmic_output_manager_v1` 3,
    `zwlr_output_power_manager_v1`, `ext_session_lock_manager_v1`,
    `ext_data_control_manager_v1` and `zwlr_data_control_manager_v1` 2,
    `zwp_input_method_manager_v2`, `zwp_virtual_keyboard_manager_v1`,
    `zcosmic_keyboard_layout_manager_v1`, `zcosmic_overlap_notify_v1`, the
    `ext-image-capture-source`/`ext-image-copy-capture` screen capture set,
    `cosmic_a11y_manager_v1` 2, `cosmic_corner_radius_manager_v1` 2, and
    `cosmic_session_lock_layer_manager_v1`. Not offered: `wlr-virtual-pointer`,
    `xdg-system-bell`, KDE blur, and any gesture protocol.
    `zcosmic_toplevel_info_v1`'s states are maximized, minimized, activated,
    fullscreen, and sticky (`cosmic-protocols`
    `unstable/cosmic-toplevel-info-unstable-v1.xml`): there is no "urgent".

### Installing COSMIC on Zorin OS 18 and Ubuntu

- **Where packages exist.** Ubuntu's archive has no COSMIC source package in
  any series (Launchpad `getPublishedSources` for `cosmic-session`,
  `cosmic-comp`, `cosmic-desktop`, `cosmic-epoch`: `total_size 0`), and Debian
  has none (<https://sources.debian.org/api/src/cosmic-comp/> and the other
  three: 404). System76's own repository `https://apt.pop-os.org/release`
  has `jammy`, `noble`, `resolute` (26.04), and `impish`, signed
  (`InRelease`, key `63C4 6DF0 140D 7389 6142 9F4E 204D D8AE C33A 7AFF`, the
  key `pop-keyring` installs). `noble` and `resolute` carry Epoch 1.11.0
  (`cosmic-session 1.11.0~1795032854`, `cosmic-comp 0.1~1795167359~6968c5d`,
  built 2026-11-20). Building from source means about 30 Rust repositories
  (`cosmic-session`'s `Depends` list) per release.
- **Ubuntu 22.04 cannot.** Pop's `jammy` has the 1.11.0 `cosmic-session`
  meta package but `cosmic-comp 0.1~1737632446` (built 2025-01-23, an alpha)
  and `xdg-desktop-portal-cosmic 0.1.0pop1~1741530540` (2025-03-09), and
  Pop's `noble` builds need `libc6 (>= 2.39)` where 22.04 has 2.35.
- **Pop's repository replaces base packages if added as is.** Of its 903
  `noble` packages, 566 are newer than what this Zorin 18 host would install
  (`pop-noble-repo-shadows-host-packages.tsv`, from `apt-cache policy` with
  `LC_ALL=C`): systemd (28 binary packages, `255.4-1ubuntu8.19pop0`), Mesa (15,
  26.1.6 against the host's 25.2.8), PipeWire (22, 1.6.8 against 1.0.5),
  Wayland (8, 1.26.0 against 1.22.0), libdrm, XWayland (24.1.13 against
  23.2.6), the kernel (`linux-system76` 7.2), NVIDIA drivers, Firefox,
  Thunderbird, flatpak, and at-spi2-core. Added without a pin, an
  `apt upgrade` would turn a Zorin machine into a half-Pop one.
- **With a pin it resolves against the host.** Taking from Pop only the
  packages the host has no candidate for, the closure of `cosmic-session`,
  `xdg-desktop-portal-cosmic`, and `cosmic-greeter` is 26 packages, every
  other dependency (including `greetd`) is satisfied by the host's own
  versions, and no versioned dependency fails
  (`cosmic-closure-on-host-noble.txt`, by a script over Pop's `Packages` and
  the host's `apt-cache policy`). This is the declared metadata only: the
  binaries were built against Pop's newer Wayland, Mesa, and PipeWire, and
  whether they run on Zorin's was **not run**.
- **The display manager.** `cosmic-session` depends on `cosmic-greeter`,
  which pre-depends on `greetd` and runs the standard Debian display-manager
  choice in its postinst (`shared/default-x-display-manager`, then
  `display-manager.service` is relinked; read from
  `/var/lib/dpkg/info/cosmic-greeter.postinst` in the VM). Preseeding that
  question to `gdm3` keeps GDM. GDM lists sessions from
  `/usr/share/wayland-sessions/`, where COSMIC installs `cosmic.desktop`
  (`Exec=/usr/bin/start-cosmic`); GDM starting it was **not run**.
- **COSMIC writes GNOME's settings.** `cosmic-settings-daemon` syncs its theme
  into GTK's settings (1.10.0 release notes: interface font and font size now
  set by it); in the VM `org.gnome.desktop.interface gtk-theme` read
  `'Pop-dark'` and `color-scheme` `'prefer-dark'`. A person who logs into
  COSMIC and back into GNOME finds those keys changed unless they are captured
  first and put back.

### Each GNOME-specific part of Better OS on COSMIC

Sizes: **small** is one adapter or setting with tests (days), **medium** a new
backend or component piece (a week or two), **large** needs an upstream change
or a new privileged or security-reviewed service.

| Part (where it lives) | What COSMIC offers instead | How Rust would reach it | Size |
| --- | --- | --- | --- |
| The touchpad adapter extension and the shared shell adapter (`adapters/gnome-shell-touchpad/`, `adapters/gnome-shell-adapter/`, `manager_platform::shell_extensions`) | No shell extensions; nothing to relay through | Each function below goes to a protocol or config instead; the extensions are not installed on COSMIC | none (dropped) |
| Gesture pipeline and suppression of GNOME's swipes (`touchpad-gestures`, `touchpad-gesture-service`, `touchpad-session`, `SuppressBuiltInGestures`) | The compositor keeps all three- and four-finger swipes, four fingers switch workspaces, nothing is configurable or exported | Either an upstream `cosmic-comp` change (a setting to pass swipes through, or a gesture protocol), or reading the touchpad ourselves, which is ADR 0008's path C with its keylogger risk and a written security review; the actions then go to `ext_workspace_v1`, `zcosmic_toplevel_manager_v1`, and the commands COSMIC binds (`cosmic-workspaces`, `cosmic-app-library`) | **large** |
| Window attention per application (`window-attention-core`, `window-attention-service`, the `WindowAttention` relay) | One machine-wide `activation_policy` (`Focus`, `FocusIfActiveWorkspace`, `Urgent`) in `com.system76.CosmicComp`; no urgent state in `zcosmic_toplevel_info_v1`, no per-application hook | The default choice can map to `activation_policy` through cosmic-config; per-application choices need an upstream protocol | small for the default, **large** (upstream) per application |
| Defaults engine's dconf and GSettings adapters (`defaults-platform`'s `gsettings.rs`, `dconf_writer.rs`, `gvdb.rs`). Shipped manifests write `/org/gnome/desktop/applications/file-manager` (Better Files, `sessions: [gnome]`), the `better-launcher` custom keybinding and the `custom-keybindings` list (Better Launcher), and the nine `org.gnome.desktop.peripherals.touchpad` keys (Better Touchpad); default applications go through `mimeapps.list`, which COSMIC reads too | cosmic-config: one RON file per key under `~/.config/cosmic/<id>/v<N>/`, defaults under `/usr/share/cosmic/`, watched by every COSMIC process (`libcosmic` `cosmic-config/src/lib.rs`); `cosmic-settings-daemon` also serves each config on the bus (`com.system76.CosmicSettingsDaemon.Config.*`, seen) | A `cosmic-config` adapter in `defaults-platform` with the same preview, capture, apply, verify, and restore steps, rewriting one RON value and keeping the rest of the file | **medium** |
| Better Launcher's Super key and summoned panel (`launcher-platform/src/shortcut.rs`, ticket 94's Super+Space) | Super alone already opens the launcher command; `system_actions`' `Launcher` names the program | Write `Launcher: "better-launcher"` into the person's `system_actions` through the cosmic-config adapter, restored on removal; the panel can be a GPUI layer-shell surface (`WindowKind::LayerShell`, `gpui_linux` `layer_shell.rs`) anchored where the person clicked. Seen as a normal window today (screenshot 10) | small |
| Touchpad page's GNOME backend (`touchpad-platform/src/gnome.rs`, nine keys) | `cosmic-comp`'s `input_touchpad` and per-device `input_devices`: state, acceleration profile and speed, click method, disable while typing, left-handed, middle-button emulation, scroll method, natural scroll, scroll factor, tap, tap button map, drag, drag lock (`cosmic-comp-config/src/input.rs`) | A `CosmicBackend` beside `GnomeBackend`, writing `com.system76.CosmicComp`; per-device profiles become native | **medium** |
| Scroll pass-through settings (`touchpad-scroll`, ADR 0030) | `scroll_config.scroll_factor`, one factor for both axes | Write the key; the evdev grab is not needed on COSMIC, except for a horizontal factor that differs from the vertical one | small |
| Better Awake's inhibitors (`awake-service/src/logind.rs`, `sleep:idle:handle-lid-switch`) | logind locks still hold suspend and the lid; screen blanking is `cosmic-idle`'s and obeys `org.freedesktop.ScreenSaver.Inhibit` only; a fullscreen window is visible in `zcosmic_toplevel_info_v1` | The ScreenSaver backend the code already names as Phase 4, and a fullscreen provider on the toplevel-info protocol (not possible on GNOME without the shell) | small to medium |
| Better Monitor's and Better Awake's trays (`monitor-tray`, `awake-tray`, StatusNotifierItem and dbusmenu) | The status-area applet is the watcher | Nothing to change; both were seen working | none |
| Better Monitor's application grouping (`monitor-views/src/grouping.rs`) | COSMIC starts an application as `app-<id>@<random>.service` under `app.slice` (seen: `app-com.system76.CosmicFiles@XJiUBXVu.service`) | `strip_launch_suffix` strips only `-<digits>`, so the id keeps `@XJiUBXVu`; strip the systemd instance suffix too | small (a defect, not COSMIC work) |
| The system frame (`better_ui::system_frame` loads GTK and draws the frame) | Server-side decorations with COSMIC's title bar; the compositor rounds and outlines every window | GPUI already binds `zxdg_decoration_manager_v1` and asks for a client frame at creation (`gpui/src/window.rs` line 1448, `better_ui` `lib.rs` lines 64 and 82); on COSMIC, ask for `WindowDecorations::Server` instead. Kept as is, the GTK frame drew a close button only, because the GTK `button-layout` COSMIC sets is `'appmenu:close'` (screenshot 02), while COSMIC's own windows show minimize and maximize as well | small |
| Login screen watch (ADR 0033, GDM-specific) | Nothing to change while GDM stays (decision C3); with `cosmic-greeter` the watch would need a `greetd` variant | — | none, or **medium** with `cosmic-greeter` |
| Drive mounting (`drive-mounts`, `storage-platform` through UDisks2, Better Files' GVfs use) | UDisks2 2.10.1 and GVfs 1.54.4 run in the Pop image; Better Files listed the drives (screenshot 02) | Unchanged | none |
| `org.freedesktop.FileManager1` (Better Files' "show in folder" service) | `cosmic-files-applet` owns it from login (`cosmic-files-applet/src/main.rs`); Better Files logged "FileManager1 is someone else's" in the VM | Better Files cannot be the folder-showing service on COSMIC while COSMIC's applet runs; it can only be the default file manager through `mimeapps.list` | small to state, **medium** to take over |
| File search (ticket 94, Tracker 3) | Tracker is not installed on Pop's image | Install `tracker-miner-fs-3` as a dependency where it is missing | small |
| The planned top bar and taskbar (ADR 0036, tickets 150–160) | `zwlr_layer_shell_v1` for any ordinary process; focused window from `zcosmic_toplevel_info_v1` or `ext_foreign_toplevel_list_v1`; window actions from `zcosmic_toplevel_manager_v1`; workspaces from `ext_workspace_v1`; keyboard layouts from `zcosmic_keyboard_layout_manager_v1`; COSMIC's panels are listed in `com.system76.CosmicPanel`'s `entries` | The bar opens as `WindowKind::LayerShell` with an exclusive zone (ticket 160), with no shell relay and no GJS; to show notifications it must replace `cosmic-notifications` as the owner of `org.freedesktop.Notifications`; hiding COSMIC's panel is writing `entries` and restoring it | **medium** (bar) and **medium** (taskbar), against **large** on GNOME |

### What COSMIC makes simpler, and what it loses

Simpler:

- The top bar and the taskbar are ordinary Rust processes on layer-shell, with
  no GJS exception, no spawned-dock relay, and no `ReserveStrip`
  (ADR 0036's widening is GNOME's cost, not the bar's).
- Ticket 142's corners: the compositor already rounds all four corners of every
  window, GTK 3, Firefox, Electron, and Better OS alike, with one radius from
  the COSMIC theme (`radius_s`, `cosmic-theme/src/model/corner.rs`). On COSMIC
  the component reduces to choosing that radius; the compositor clip that needs
  a new shell ADR on GNOME is built in.
- The desktop: a Better Files desktop is a layer-shell surface on the
  background layer, with no shell code.
- Touchpad scroll speed and per-device settings are native settings.
- An Awake fullscreen trigger becomes readable.

Harder or lost:

- **Gestures**: the Mac-style preset cannot work on COSMIC today, because the
  compositor keeps the swipes (above).
- **Per-application window attention** has no hook.
- **GNOME's application grid**, which the owner likes: COSMIC has its own
  App Library (Super+A, `cosmic-app-library`), a different grid.
- **The owner's GNOME extensions** (`gnome-extensions list --enabled` on this
  machine): `zorin-taskbar`, `zorin-menu`, `zorin-desktop-icons`,
  `zorin-tiling-shell`, `zorin-appindicator`, `zorin-printers`, `drive-menu`,
  `user-theme`, and Better OS's own `touchpad-adapter`. None runs on COSMIC;
  COSMIC has its own dock, app library, tiling, status area, and desktop mode.
- **Zorin's own features** installed here (`dpkg-query -W 'zorin-*'`): Zorin
  Appearance and its layouts, Zorin Connect, Zorin's themes, icon themes, and
  sound theme, Windows app support, and the Zorin upgrader. Zorin Connect and
  Zorin Appearance are GNOME-side programs; Appearance's layouts are GNOME
  Shell layouts and do nothing on COSMIC (not run).
- **GNOME Online Accounts and the breadth of GNOME Settings**: COSMIC Settings
  is its own, smaller application; whether it has an online accounts page was
  not checked.
- **IBus**: replaced by Fcitx 5 for Better OS windows (decision C4).
- **Ubuntu 22.04** gets nothing.

### Desktop icons (ticket 144)

- **COSMIC Files' desktop mode is in the stable release.** `cosmic-session`
  starts `cosmic-files-applet` at every login (`src/main.rs` lines 352–353),
  which serves `FileManager1` and runs `cosmic_files::desktop()`. In the VM it
  showed `~/Desktop`'s folders and files on the desktop, with thumbnails, in a
  grid filled column by column from the top left (screenshots 02, 17). Its
  settings are `grid_spacing`, `icon_size`, `show_content`,
  `show_mounted_drives`, and `show_trash` (`src/config.rs`,
  `DesktopConfig`). It does not let icons be placed freely ("Icons not movable
  on desktop", issue 1239, open since 2025-10-03), it repeats every icon on
  every monitor (issue 569, open), and it has no stacks.
- **Zorin's `zorin-desktop-icons`** on GNOME is Zorin's JavaScript; stacks
  cannot be added to it without Better OS writing a JavaScript application,
  which AGENTS.md forbids.
- **Better Files drawing the desktop** is the one route that gives ticket 144's
  stacks, the same desktop on both desktops, and Better Files' own views and
  kind rules. On COSMIC it is a GPUI layer-shell surface on the background
  layer, and COSMIC's is turned off by writing `show_content: false` in
  `com.system76.CosmicFiles`' `desktop` entry (restored on removal). On GNOME
  it is a view the shared extension starts and makes the desktop window, as
  Zorin's extension does (ADR 0036's context names that route): a second row in
  ADR 0036's closed program table, an amendment to ADR 0036, and Zorin's
  extension disabled while it runs and re-enabled on removal.
- **Recommendation (C6):** Better Files draws the desktop. On COSMIC it can be
  built as soon as the owner chooses B or C. On GNOME it waits on ADR 0036's
  acceptance; until then GNOME keeps Zorin's extension and has no stacks.
  Either way `~/Desktop` is never moved: every option reads the same folder.

### GPUI on COSMIC

GPUI's Wayland client (`gpui_linux` `src/linux/wayland/client.rs` lines
225–266, Zed `ae394f3`, the revision `Cargo.lock` pins) needs `wl_compositor`,
`wl_shm`, and `xdg_wm_base`, and uses when offered `xdg_activation_v1`,
cursor-shape, data device, primary selection, viewporter, fractional scale,
`zxdg_decoration_manager_v1`, `zwlr_layer_shell_v1`, KDE blur,
`zwp_text_input_manager_v3`, pointer gestures, `xdg_wm_dialog_v1`, and
`xdg_system_bell_v1`. `cosmic-comp` offers all of them except KDE blur and the
system bell, so GPUI loses only a blurred background and the bell. In the VM
(software rendering through Mesa's lavapipe and llvmpipe, so nothing about
speed can be judged) every window drew, scaled, took input, and opened
COSMIC's own file chooser through the portal (screenshot 14).

## Options and the work each needs

### A. Stay on GNOME

No new work. Tickets 142 (items 2 and 3), 144, 151–159 continue as written,
each on ADR 0036 or a further shell ADR. Ticket 160 is withdrawn. COSMIC is
not offered.

### B. Support both, GNOME the default

1. **Desktop-neutral pieces first** (they are right whichever desktop runs):
   Better Monitor's `@instance` grouping fix (small); Tracker as a dependency
   where it is missing (small).
2. **The cosmic-config adapter** in `defaults-platform`, with capture and
   restore (medium).
3. **COSMIC backends:** Touchpad page (medium), scroll factor (small),
   launcher key (small), default window-attention choice (small), server-side
   frame on COSMIC (small), Better Awake's ScreenSaver backend and fullscreen
   provider (small to medium), and
   honest "not on COSMIC" states for gestures and per-application attention,
   which already read well (screenshots 04, 07).
4. **A `better-desktop-cosmic` component**, the one action the owner asked
   for, through `manager-daemon` (each step is a privileged operation with its
   polkit action and an undo):
   - add System76's source, keyring, and the pin of decision C2 (medium,
     plus the ADR that rule needs);
   - preseed the display manager to `gdm3` and install the 26 packages
     (medium);
   - install `fcitx5`, `fcitx5-chewing`, and its GTK front ends, and carry the
     person's IBus engines over where Fcitx 5 has the same one (medium);
   - set the person's session to `cosmic` in AccountsService, which GDM reads
     (small);
   - capture GNOME's interface keys that `cosmic-settings-daemon` overwrites,
     and carry over to cosmic-config the wallpaper, dark or light mode, keyboard
     layouts, display scale, and the dock's pinned applications (medium);
     `mimeapps.list` and `~/Desktop` are shared and untouched;
   - offer "Log out now" as part of the same action, since a desktop changes
     only at the next login;
   - switching back sets the previous session and puts the captured GNOME
     keys back; removal also purges the 26 packages, the source, keyring, and
     pin, and deletes COSMIC's per-person settings unless "Also delete my
     settings and history" is unticked (medium);
   - a VM check on Zorin OS 18 and Ubuntu 24.04 of install, login through
     GDM, switching back, and removal, the part this ticket could not run
     (medium).
5. **Ticket 160** (the bar on COSMIC) and a COSMIC taskbar row in ticket 142,
   each medium.
6. **Not offered on Ubuntu 22.04.**

### C. COSMIC the primary desktop

Everything in B, plus:

- the gesture pipeline on COSMIC (large: an upstream change, or reading the
  touchpad ourselves after a written security review);
- per-application window attention (large, upstream);
- Better OS's taskbar and top bar replacing COSMIC's panel and dock by default
  (medium each, on layer-shell);
- deciding what happens to the GNOME code for people who stay on Zorin or
  Ubuntu with GNOME (keep it, doubling maintenance, or drop it, dropping 22.04
  entirely);
- for ticket 76's Debian distribution: Debian has no COSMIC, so Better OS would
  build about 30 source packages per COSMIC release and publish them from its
  own signed repository (large, and the signing decision AGENTS.md defers).

### What each option does to the tickets

| Ticket | A | B | C |
| --- | --- | --- | --- |
| 142 (theme, corners, taskbar) | As written; item 2 needs a shell ADR, item 3 ADR 0036's row | GNOME as written; on COSMIC the corners are done by the compositor and the taskbar is a layer-shell process | COSMIC part only; GNOME part kept or dropped with the GNOME code |
| 144 (desktop stacks) | Waits on ADR 0036 and a desktop row | Better Files' desktop on COSMIC first, GNOME after ADR 0036 | Better Files' desktop on COSMIC |
| 94 (Super+Space, file search) | As written | Also writes COSMIC's `system_actions`; Super+Space is COSMIC's input-source switch, so it moves there too; Tracker installed where missing | Same as B |
| 95 and 150–160 (top bar) | 151–159 as written, 160 withdrawn | All of them; 151 and 159 are GNOME-only | 152–158 and 160; 151 and 159 only if GNOME is kept |
| 99 (Better Input) | IBus and Fcitx 5 both | Must work under Fcitx 5 on COSMIC | Fcitx 5 first |
| 139, 140 (GDM) | As written | As written while GDM stays | As written while GDM stays |
| 76 direction 3 (own Debian distribution) | GNOME is in Debian | Same as A, COSMIC an extra | Better OS builds and signs COSMIC itself |

## Risks

- **Runtime compatibility on Zorin's base is unproved.** The packages declare
  nothing newer than noble has, but were built against Pop's newer Wayland,
  Mesa, and PipeWire; a missing symbol shows only when it runs.
- **A third-party repository on the person's machine.** Its key signs System76's
  kernel and systemd too; the pin is what keeps those off the machine, and a
  pin written wrong replaces the base system on the next upgrade.
- **COSMIC moves fast.** Twelve releases in eleven months, and a behaviour
  Better OS relies on (the decoration default changed on 2026-11-06) can
  change without notice.
- **Upstream gaps** (gestures, attention, IBus) are not Better OS's to close
  quickly.
- **Two desktops double every desktop-facing test**, and the evidence for this
  ADR is one VM with software rendering, a live session, and no GDM.

## Recommendation

**B, with GNOME the default**, in this order: the desktop-neutral pieces, the
cosmic-config adapter and the COSMIC backends, the bar and the desktop on
COSMIC, and the one-click component last, after its VM check on Zorin OS 18
passes. Reasons:

- The owner's own machine is Zorin with GNOME, with the GNOME grid and six
  Zorin extensions in use, IBus-Chewing for Chinese, and the Mac-style
  gestures Better Touchpad is built for. C takes the gestures and the grid away
  today and depends on upstream to give them back.
- COSMIC does make the owner's newest requests far cheaper (uniform corners,
  a top bar and taskbar in Rust, a desktop drawn by Better Files), and every
  Better OS window already runs on it, so offering it costs one component and
  a set of backends, not a rewrite.
- The one-click switch meets the owner's rule only with a pinned third-party
  source, which needs C2, and only on 24.04 bases; offering it before a pinned
  install has run on Zorin would ship the one step nobody has seen.
- For the Debian distribution, GNOME is packaged by Debian and COSMIC is not;
  B keeps that direction's cost where it is.

## What was run

On 2026-11-21, in a disposable VM (QEMU 8.2.2 with KVM, 4 vCPUs, 6 GiB,
under `systemd-run --user --scope -p MemoryMax=7G -p MemorySwapMax=0`, no
disk image), booted from System76's Pop!_OS 24.04 build 29 ISO, downloaded
over HTTPS from `iso.pop-os.org` and checked against its published SHA-256
(`49567ae9…a4a067d0b7`). Better OS 0.2.16's published Ubuntu 24.04 amd64
packages (Files, Git, Monitor, Manager, Launcher, Awake), whose glibc and
library needs match the noble-based guest, were installed in the live session
instead of building the five windows here: the host's disk could not hold the
build and the ISO together. The VM was ended by its PID and the ISO, kernel,
and packages deleted (about 3.4 GB freed). Screenshots and command outputs
are in `/home/tim/Developer/better-os-evidence/143/`, outside the repository:

| File | What it shows |
| --- | --- |
| `01-cosmic-live-desktop.png` | The COSMIC session with the panel, dock, and Pop's installer |
| `02-files-100.png`, `02a-…`, `02b-…` | Better Files at 100 %: GTK frame with a close button only, all four corners rounded by the compositor, COSMIC Files' desktop icons behind it |
| `03-manager-100.png`, `04-…`, `07-…` | Better Manager: home, the Touchpad page, and Window attention saying GNOME Shell is not running |
| `05-panel-status-area-trays.png`, `06-awake-tray-menu.png` | Better Awake's and Better Monitor's tray icons in COSMIC's panel, and Better Awake's menu |
| `08-git-100.png`, `13-…`, `14-…` | Better Git, its folder dialog, and COSMIC's file chooser opened through the portal |
| `09-monitor-100.png` | Better Monitor |
| `10-launcher-100.png`, `11-…`, `12-…` (and crops `11a`, `12a`) | Better Launcher as a normal window, and Zhuyin through Fcitx 5 in its search field, composing and committed |
| `15-files-200.png`, `16-manager-150.png` | Better Files at 200 % and Better Manager at 150 % |
| `17-cosmic-files-desktop-mode-context-menu.png` | COSMIC Files' desktop mode with `~/Desktop`'s contents |
| `wayland-info-and-portals.txt`, `session-and-config.txt` | Every global `cosmic-comp` advertised, the installed COSMIC versions, the portal's interfaces, the session bus names, COSMIC's shortcut and touchpad defaults |
| `pop-noble-repo-shadows-host-packages.tsv`, `cosmic-closure-on-host-noble.txt` | The 566 packages Pop's `noble` would replace on this host, and the 26-package pinned closure |

Not run: a light-mode pass; COSMIC installed onto Zorin OS 18 or Ubuntu 24.04
with the pin; GDM starting a COSMIC session; switching back and removing; real
hardware; any touchpad.
