# ADR 0038: Better OS's desktop — stay on GNOME, add COSMIC, move to COSMIC, or a Better OS session on COSMIC's compositor

## Status

Decided by the owner on 2026-11-21: **option D**, a Better OS session on
COSMIC's compositor, beside GNOME. It was proposed that day for ticket 143 and
revised the same day for the owner's fourth option and the gesture
requirement. Decisions C2 to C9 below are not answered yet; until each is,
the work that depends on it is not built. The tickets for option D are
written from this record.

## The owner's decisions

| # | Decision | Recommendation |
| --- | --- | --- |
| C1 | Which option: **A** stay on GNOME; **B** support both, GNOME the default, COSMIC as a second desktop; **C** COSMIC the primary desktop; **D** a Better OS session of its own on COSMIC's compositor, beside GNOME | **D**, beside GNOME, which stays the installed default until the Better OS session has passed its VM check and a week on the owner's machine. Build now only what carries over to D; hold the GNOME-only relays (the list under "Which of tickets 150 to 161 should wait") |
| C2 | Whether Better Manager may add System76's APT source, its signing key, and an APT pin to a person's machine (AGENTS.md: no public APT repository or signing implementation without a decision) | **Yes, narrowly**: one source file, one keyring holding the one fingerprint below, and a pin that takes only the named COSMIC packages from it (14 for D, 26 for B or C). The alternative, Better OS building those packages itself, is in option D's work |
| C3 | The login screen: keep GDM, or let `cosmic-greeter` take over | **Keep GDM**; it lists any Wayland session, and ADR 0033's watch keeps working |
| C4 | Input method outside GNOME: Fcitx 5 with `fcitx5-chewing` in place of IBus | **Yes**; IBus 1.5.29 offers `cosmic-comp` no Wayland input method, Fcitx 5 was seen typing Zhuyin into a Better OS window |
| C5 | Better OS windows' frame outside GNOME: the GTK frame they draw today, or the compositor's title bar | **The compositor's** (server-side), so they match every other window |
| C6 | Who draws the desktop's icons (ticket 144) | **Better Files**: a layer-shell surface outside GNOME; on GNOME a second row in ADR 0036's program table, replacing Zorin's extension while it runs, built only if the owner keeps building GNOME-only surfaces (option A or B) |
| C7 | COSMIC's own panel while Better OS's top bar runs (options B and C) | **Hidden, and put back exactly**; in option D there is no COSMIC panel to hide |
| C8 | (D) Run `cosmic-comp` as System76 releases it and wait for upstream's gesture settings, or keep a Better OS patch that hands swipes to Better OS | **Unmodified first.** Add the one gesture patch, shipped as a Better OS package, only when the rest of the session works and upstream still has none. Upstream refuses code written by a language model (its pull request template), so a patch Better OS's agents write stays Better OS's |
| C9 | (D) Display, network, Bluetooth, sound, users, and region settings in the Better OS session | **COSMIC Settings** inside the session at first; moving each page into Better Manager (ADR 0017) is later work with its own tickets |

## Context

On 2026-11-16 the owner asked whether Better OS should move to COSMIC,
System76's Rust desktop, and set the bar for any move: once Better OS is
installed, one action in Better Manager installs COSMIC and switches the person
to it, with nothing else to set up, and removing it puts the previous desktop
back. Better OS today is written against GNOME 46 on Zorin OS 18 and Ubuntu.

On 2026-11-21, told that on GNOME nothing but the shell can draw a panel, the
owner asked again whether Better OS should make its own desktop, and added a
requirement: **every gesture has the transition macOS has**: the result
follows the fingers, can be turned back halfway, and settles when they lift.
Since the first evaluation, the work reached on GNOME only through shell code
has grown: the taskbar and top bar (ticket 142, tickets 151 to 159, ADR 0036),
a notification island and a right-edge notification center (ticket 161), one
corner radius for every window (ticket 142), the desktop and its stacks
(ticket 144), finger-following gestures, and the notification center's edge
swipe (ticket 161, N3). The owner named a fourth option: **a Better OS session
on COSMIC's compositor** (`cosmic-comp`, Rust and GPL-3.0 like Better OS),
with Better OS's panels, launcher, notifications, and gesture transitions as
Rust processes on layer-shell and no GJS. Writing a compositor from nothing is
excluded by the ticket.

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
- **COSMIC keeps its GTK settings apart from GNOME's.** `start-cosmic` sets
  `DCONF_PROFILE=cosmic`, and that profile is `user-db:cosmic` then
  `user-db:user` (`pop-os/cosmic-session`, `data/start-cosmic` and
  `data/dconf/profile/cosmic`): inside a COSMIC session every GSettings write,
  `cosmic-settings-daemon`'s theme and font syncing included, goes to
  `~/.config/dconf/cosmic`, and GNOME's own database is only read. (The VM's
  `gtk-theme 'Pop-dark'` was read from a serial shell outside that profile,
  so it says nothing about a person's GNOME values.) One consequence for
  Better OS: `defaults-platform` always writes `/ca/desrt/dconf/Writer/user`
  and reads `~/.config/dconf/user` (`dconf_writer.rs`, `dconf.rs`), so a
  GNOME key it writes from a COSMIC session lands in GNOME's database and is
  hidden there by any value the `cosmic` database holds.

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
  built in options B, C, and D, and in D it is the session's only desktop. On
  GNOME (A, B) it needs an amendment adding a desktop row to ADR 0036, which
  the owner accepted on 2026-11-21 for the bar alone; until then GNOME keeps
  Zorin's extension and has no stacks, and under D that row waits like
  ticket 151. Either way `~/Desktop` is never moved: every option reads the same folder.

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


### The fourth option: a Better OS session on `cosmic-comp`

A session of Better OS's own: `cosmic-comp` as the compositor, the COSMIC
daemons Better OS does not replace, and Better OS's top bar, taskbar,
launcher, notification island and center, desktop, and gesture transitions as
Rust processes on layer-shell. Sources as above (`cosmic-comp` master
`6968c5d`, 2026-11-20, unless a line says otherwise); no VM was needed for
this part and none was run.

#### What `cosmic-comp` allows from outside, and what needs a change inside it

| Piece | From outside today | Inside `cosmic-comp` |
| --- | --- | --- |
| Top bar, taskbar, notification island and center | Layer-shell surfaces for any client outside a security context (`state.rs` line 744, `client_not_sandboxed`); the focused window, every window, and window actions through `zcosmic_toplevel_info_v1`, `ext_foreign_toplevel_list_v1`, and `zcosmic_toplevel_manager_v1`; workspaces through `ext_workspace_v1`. To show notifications Better OS owns `org.freedesktop.Notifications` itself, which is simple in its own session: `cosmic-notifications` is not started (in COSMIC's session `cosmic-session` starts it beside the panel, `src/main.rs` lines 270–310) | Nothing |
| Desktop surface and stacks (ticket 144) | A layer-shell surface on the background layer, one per output (GPUI below); COSMIC Files' desktop process is not started in Better OS's session, so Better Files also owns `org.freedesktop.FileManager1` there | Nothing |
| One corner radius for every window (ticket 142) | Already done: every floating and tiled window is clipped to the theme's `radius_s` (plus 4 px above 4), maximized windows excepted (`src/shell/element/window.rs`, `corner_radius`; `AppearanceConfig` defaults true). The radius is COSMIC theme configuration (`com.system76.CosmicTheme.*`, `corner_radii`), written through cosmic-config | Nothing, unless the radius must differ from the theme's small radius |
| Window title bars in Better OS's style | The compositor draws server-side title bars with libcosmic | `cosmic_comp::run(Hooks)` takes compile-time hooks for window and stack decorations (`src/hooks.rs`, "an _unstable_ interface"): a Better OS compositor binary linking `cosmic-comp` as a library could draw its own title bars without a patch, in libcosmic (iced), not GPUI |
| Finger-following workspace switch | Already, for four fingers: the compositor feeds the swipe's delta into `update_workspace_delta` while the fingers move and settles with their velocity in `end_workspace_swipe` (`src/input/mod.rs` lines 1294–1350); it can be turned back halfway | To use three fingers, or Better OS's own directions: a gesture setting (the open pull requests below do this) |
| Finger-following overview, launcher, notification center, or any transition Better OS defines | Not possible: every swipe of three or more fingers stays in the compositor (`src/input/mod.rs` lines 1195–1210 set `gesture_state` for any count ≥ 3, and the update and end handlers then never forward the event); three fingers do nothing ("TODO: 3 finger gestures"); no protocol hands a swipe to an unfocused client. COSMIC's own overview (`cosmic-workspaces`) opens without following the fingers | A patch: a private protocol through which the session's gesture service, a non-sandboxed client, claims swipes of a given finger count and receives begin, update (finger count and delta), and end (cancelled or not); and a request to drive the workspace switch with the existing `update_workspace_delta` and `end_workspace_swipe`. It touches `src/input/mod.rs` (the three swipe arms), a new module under `src/wayland/protocols/` and its handler, one line in `state.rs`, and one protocol XML: about 400 to 800 lines, an estimate from the size of the open gesture pull requests (+191/−47 to +488/−411). The overview, the launcher, and the notification center then follow the fingers inside Better OS's own processes, which draw them |
| The notification center's edge swipe (ticket 161, N3) | Not from any Wayland protocol | Not inside the compositor either: for a touchpad, libinput reports a swipe as the movement of the fingers' logical centre and passes individual touch points only for touchscreens (<https://wayland.freedesktop.org/libinput/doc/latest/gestures.html>, "Swipe gestures" and "Touchscreen gestures"), and `cosmic-comp` reads input through libinput. Where the fingers are on the pad is read from the evdev node, as Better Touchpad's scroll pass-through does (ADR 0030): ticket 161's option (a), the same in every option of this ADR |

**Upstream's plans for gestures.** COSMIC's roadmap lists "Customizable
touchpad gestures" and a "Workspaces animation" item under Epoch 3, after
Epoch 2, with no dates, each epoch "roughly six to eight months"
(<https://system76.com/blog/post/cosmic-epoch-2-and-3-roadmap>, published
2026-02-04). Issue 315, "Touchpad: Navigation Gestures", is open since
2024-02-19 and was pointed at Epoch 3 on 2026-02-18
(<https://github.com/pop-os/cosmic-comp/issues/315>). Six pull requests that
make gestures configurable or add gestures are open and unmerged: 396
(2024-04-01, a gesture configuration backend), 1109, 1799, 2824, 2838, and
3054 (2026-11-10); 2252 and 2386 were closed. A comment on 1799 quotes the
maintainers preferring to "rework and merge" 396 before adding gestures
(<https://github.com/pop-os/cosmic-comp/pull/1799>). Each maps a gesture to
an action that fires once (or to the existing workspace swipe); none hands
progress to an outside process. Small outside pull requests do merge (794,
natural four-finger direction, merged 2024-08-30; 2215, workspace wrapping,
merged 2026-04-14).

#### What a fork costs

- **Pace.** 503 commits reached `cosmic-comp`'s default branch between
  2025-12-10 and 2026-11-20 (GitHub's commits listing), 61 of them in
  `src/input/mod.rs` and 34 in `src/state.rs`, the two files a gesture patch
  touches; Epoch releases came every one to three weeks. A patch there
  conflicts often enough that it must be rebased and rebuilt for every
  release, and Better OS would ship `cosmic-comp` as its own package from then
  on (built in ADR 0021's release environments, for 24.04; whether it builds
  on 22.04 was not tried).
- **Size.** One patch of an estimated 400 to 800 lines for gestures. Nothing
  else on the list above needs one; title bars in Better OS's style are a
  library build with hooks, not a patch.
- **Would upstream take it.** Configurable gestures are wanted upstream, but
  the open pull requests have waited up to 31 months, and COSMIC's pull
  request template (`pop-os/cosmic-epoch`, `.github/PULL_REQUEST_TEMPLATE.md`)
  requires the author to certify "I have not included any LLM (also known as
  AI) generated content in this PR", and closes pull requests without the
  checkbox. Better OS's code is written by agents, so nothing Better OS
  writes can be offered upstream unless a person writes it again by hand.
- **Licence.** `cosmic-comp` is `GPL-3.0-only` (`src/main.rs`'s SPDX line,
  GitHub's licence field `GPL-3.0`); Better OS is GPL-3.0-or-later (ADR 0003).
  A patched compositor stays GPL-3.0-only and its source must be offered with
  the package, which is the Corresponding Source question AGENTS.md already
  leaves to the owner. Better OS's own processes only talk to it over Wayland
  and D-Bus and are not affected.
- **No fork.** Run the released compositor, which already gives the
  four-finger workspace switch that follows the fingers and settles, and wait
  for Epoch 3. While waiting Better OS loses: three-finger gestures, an
  overview, launcher, or notification center that follows the fingers, and
  any gesture of Better OS's own design. Gestures that fire at the end (the
  state on GNOME today) cannot be had either, because the compositor keeps
  the swipes.

#### What the session needs that GNOME gives today

| Need | Provided by | Maturity (evidence) |
| --- | --- | --- |
| Starting the session | A Better OS session file in `/usr/share/wayland-sessions/` and a small launcher: `cosmic-comp <program>` runs the program once it is up (`src/lib.rs` lines 125–147), and that program starts a systemd user target with the services below. `cosmic-session` cannot be reused: its list of components is fixed in code (`src/main.rs` lines 270–356) | New Better OS work, medium |
| Settings daemon (theme, fonts, GTK and Qt sync, brightness and input-source keys) | `cosmic-settings-daemon` | Stable 1.11 in Pop (seen running) |
| Portals (file chooser, screenshot, screen cast, remote desktop, settings) | `xdg-desktop-portal-cosmic`; `XDG_CURRENT_DESKTOP` must include `COSMIC` for its `UseIn=COSMIC`, plus GTK's portal for what COSMIC lacks (Pop's `cosmic-portals.conf`: `default=cosmic;gtk;`) | Stable; the file chooser was seen opening for a GPUI window |
| Polkit agent | `cosmic-osd` (`src/subscriptions/polkit_agent.rs`, `src/components/polkit_dialog.rs`), which also draws volume and brightness pop-ups | Stable 1.11 |
| Keyring | `gnome-keyring` and `oo7` through the Secret portal (Pop's `cosmic-portals.conf`: `Secret=oo7-portal;gnome-keyring;`) | GNOME's own, unchanged |
| Screen lock and idle | `cosmic-greeter` as the locker (`ext_session_lock_manager_v1`), `cosmic-idle` for blanking and suspend, `cosmic-bg` for the wallpaper, also on the lock screen (1.9.0) | Stable; both seen running |
| Login screen | GDM (decision C3), which lists the session file | GDM starting a non-GNOME session was not run |
| Input methods for Chinese | Fcitx 5 with `fcitx5-chewing`, started by the session, over `zwp_input_method_v2` and `text-input-v3` | Seen working in a Better OS window; IBus does not serve this compositor |
| Network and Bluetooth prompts | In COSMIC they live in the panel's applets and COSMIC Settings (NetworkManager secret agent code in `cosmic-settings` `pages/networking/` and `cosmic-applet-network`; BlueZ agent bindings in `pop-os/dbus-settings-bindings`, `bluez/src/agent1.rs`; read, not run). Without COSMIC's panel, Better OS's status area (ticket 157) must register its own NetworkManager secret agent and BlueZ agent, or run COSMIC Settings' | **Medium**, new Better OS work |
| System settings pages | COSMIC Settings (decision C9) | Stable |
| XWayland | `cosmic-comp` starts it (seen, rootless) | Stable |
| Accessibility | Orca with AT-SPI, `cosmic_a11y_manager_v1` (magnifier, filters) | Shipped; depth not assessed |
| Screen sharing | `xdg-desktop-portal-cosmic` ScreenCast and RemoteDesktop over `ext-image-copy-capture` and PipeWire | Shipped in Pop with PipeWire 1.6.8; on noble's PipeWire 1.0.5 not run |
| Tray watcher, notifications, launcher, app grid, desktop | Better OS (top bar's tray host, ticket 161's island and center, Better Launcher, Better Files) | Better OS's own tickets |

#### What carries over unchanged and what stays behind on GNOME

- **Carries over:** every GPUI window (`files-gui`, `git-gui`, `monitor-gui`,
  `manager-gui`, `launcher-gui`, `awake-gui`, `touchpad-gui`,
  `app-chooser-gui`; five were seen on `cosmic-comp`); every service
  (`manager-daemon`, `storage-service`, `monitor-service`, `awake-service`,
  `touchpad-gesture-service`'s recognizer, `touchpad-scroll`, the hardware
  helpers); both trays (`awake-tray`, `monitor-tray`); `drive-mounts`; the
  defaults engine's XDG and `mimeapps.list` adapters; the top bar's service
  and view (ticket 152), which open as `WindowKind::LayerShell` instead of a
  dock the shell made (ticket 160); ADR 0033's login screen watch while GDM
  stays.
- **Stays behind (GNOME only):** `adapters/gnome-shell-touchpad/`,
  `adapters/gnome-shell-adapter/` and every relay in it (window attention,
  ADR 0036's panel relay), `manager_platform::shell_extensions`,
  `touchpad-session`'s adapter bus, `touchpad-platform/src/gnome.rs`, the
  dconf and GSettings adapters in `defaults-platform` (`gsettings.rs`,
  `dconf_writer.rs`) for GNOME's keys, `launcher-platform/src/shortcut.rs`'s
  GNOME keybinding, `better_ui::system_frame`'s GTK frame (the compositor
  draws title bars), `hardware-platform`'s `gnome-settings-daemon` endpoints
  (UPower's keyboard backlight endpoint is already its fallback), and the
  window-attention service's per-application choices, which have no
  equivalent (above).

#### GPUI's layer-shell surface at the pinned revision

`crates/gpui/src/platform/layer_shell.rs` and
`crates/gpui_linux/src/linux/wayland/window.rs` (Zed `ae394f3`) offer:
`WindowKind::LayerShell(LayerShellOptions)` with a namespace, the four layers
(background, bottom, top, overlay), any combination of the four edge anchors,
an exclusive zone and its edge (also changeable after creation:
`Window::set_exclusive_zone`, `set_exclusive_edge`), margins, keyboard
interactivity (none, exclusive, on demand), the output chosen by `display_id`,
an input region (`set_input_region`), and popups parented to a layer surface
(`get_popup` on the layer surface). It refuses with
`LayerShellNotSupportedError` where the compositor has no layer-shell, which
is why it cannot run on GNOME. A Better OS panel would still need: one window
per output and following outputs as they come and go (GPUI opens one surface
per call), changing anchors, margins, or the layer after creation (not
exposed), and its own slide and follow-the-fingers animation drawn inside a
fixed surface with the input region trimmed to what is visible. **Nobody has
run a Better OS window as a layer-shell surface**; in the VM Better Launcher
ran as a normal window.

#### One action in Better Manager, and removal

- **Install** (`better-os-session` component, through `manager-daemon`):
  System76's source, key, and pin (decision C2) for the 14 packages the
  session needs (`cosmic-comp`, `cosmic-settings-daemon`,
  `xdg-desktop-portal-cosmic`, `cosmic-osd`, `cosmic-idle`, `cosmic-greeter`
  and its daemon, `cosmic-bg`, `cosmic-randr`, `cosmic-settings`,
  `cosmic-icons`, `cosmic-theme-bundles`, `adw-gtk3`, `pop-sound-theme`;
  every other dependency resolves to the host's own noble versions,
  `session-closure-on-host-noble.txt`), with the display manager question
  preseeded to `gdm3`; Fcitx 5 and Chewing from Ubuntu's archive; Better OS's
  session package (session file, launcher, systemd user target, the session's
  configuration of COSMIC's daemons); the person's AccountsService session set
  to it; and "Log out now" in the same action. If decision C8 adds the
  gesture patch, `cosmic-comp` then comes from Better OS's own release assets
  instead of System76's repository, like every other Better OS package; and
  Better OS could build all 14 itself the same way and drop C2, at the cost
  of building about ten Rust repositories per COSMIC release.
- **Removal:** the previous session back in AccountsService, the packages,
  source, key, and pin purged, GDM untouched; Better OS's session settings
  deleted unless "Also delete my settings and history" is unticked. GNOME's
  dconf database is never written by COSMIC's daemons (their profile writes
  to `~/.config/dconf/cosmic`, above), so GNOME comes back as it was; COSMIC's
  database goes with the settings.
- **Without the pin** Pop's repository would replace 566 of the host's
  packages, the kernel and systemd among them; **on Ubuntu 22.04** there is
  nothing to install (Pop's `jammy` compositor is the January 2025 alpha), so
  option D, like B and C, is offered on 24.04 bases only.
- **Not run:** any of this on Zorin OS 18 or Ubuntu 24.04, and GDM starting
  the session. It is option D's first check.

### Gestures that follow the fingers, on each option

- **A (GNOME 46).** GNOME's own trackers already follow the fingers for the
  overview (vertical) and workspaces (horizontal), but Better OS takes them off
  (`SuppressBuiltInGestures`) to run the Mac-style preset, whose actions fire
  at the end. For Better OS's gestures to follow the fingers, JavaScript in
  the shell must take progress values from Better OS and call
  `Main.overview._gestureBegin`, `_gestureUpdate`, and `_gestureEnd`
  (`js/ui/overview.js`) and `WorkspaceAnimationController`'s
  `_switchWorkspaceBegin`, `_switchWorkspaceUpdate`, and `_switchWorkspaceEnd`
  (`js/ui/workspaceAnimation.js`, reached as `Main.wm._workspaceAnimation`),
  passing an object shaped like the shell's `SwipeTracker`, because those
  methods set `tracker.orientation` and call `tracker.confirmSwipe(distance,
  snapPoints, progress, cancelProgress)`. The names are the same in GNOME
  Shell 45.0, 46.0, 47.0, 48.0, and 49.0, but they are private (leading
  underscore) and their bodies change: 46 dropped the
  `Meta.disable_unredirect_for_display` call 45 made in `_gestureUpdate`, and
  49 returns early when progress is 0 (read from `GNOME/gnome-shell` at those
  tags on 2026-11-21). This breaks ADR 0018's bounds in three places: it
  calls methods the shell does not expose as actions, it keeps an object
  with state between calls (the tracker stand-in), and it relays a stream of
  progress values, one per frame, from Rust over the session bus, which adds
  a bus round trip to every frame. It needs a new ADR, and its own
  measurements of the added latency. The cheaper alternative on GNOME is to
  leave GNOME's own swipes on for the two transitions GNOME animates and give
  up the Mac preset's directions for them. The island, the center, and the
  desktop are Better OS windows on GNOME (ADR 0036's docks) and can follow
  the fingers themselves, given the progress.
- **B and C (COSMIC as System76 ships it).** The four-finger workspace switch
  follows the fingers and settles; nothing else does, and nothing can be
  added from outside until upstream's gesture settings arrive (Epoch 3), and
  even those, as proposed, fire actions rather than hand over progress.
- **D (Better OS's session).** The workspace switch as in B; with decision
  C8's patch, every other transition (overview, launcher, notification
  center, desktop stacks) is drawn by Better OS's own layer-shell processes
  from the progress the gesture protocol delivers, with no shell code. The
  edge swipe comes from the evdev pass-through in every option.

## Options and the work each needs

### A. Stay on GNOME

No new desktop. Tickets 142 (items 2 and 3), 144, 151–159, and 161 continue
on ADR 0036 and further shell ADRs: compositor rounding for every window,
the island and center as more docks, a desktop row, and, for gestures that
follow the fingers, JavaScript driving the shell's private transitions (above).
Ticket 160 is withdrawn. COSMIC is not offered.

### B. Support both, GNOME the default, COSMIC a second desktop

1. **Desktop-neutral pieces first:** Better Monitor's `@instance` grouping fix
   (small); Tracker as a dependency where it is missing (small).
2. **The cosmic-config adapter** in `defaults-platform`, with capture and
   restore (medium).
3. **COSMIC backends:** Touchpad page (medium), scroll factor (small),
   launcher key (small), default window-attention choice (small), server-side
   frame on COSMIC (small), Better Awake's ScreenSaver backend and fullscreen
   provider (small to medium), and honest "not on COSMIC" states for gestures
   and per-application attention, which already read well (screenshots 04,
   07).
4. **A `better-desktop-cosmic` component**, through `manager-daemon` (each
   step a privileged operation with its polkit action and an undo):
   - add System76's source, keyring, and the pin of decision C2 (medium, plus
     the ADR that rule needs);
   - preseed the display manager to `gdm3` and install the 26 packages
     (medium);
   - install `fcitx5`, `fcitx5-chewing`, and its GTK front ends, and carry the
     person's IBus engines over where Fcitx 5 has the same one (medium);
   - set the person's session to `cosmic` in AccountsService, which GDM reads
     (small);
   - carry over to cosmic-config the wallpaper, dark or light mode, keyboard
     layouts, display scale, and the dock's pinned applications (medium);
     `mimeapps.list` and `~/Desktop` are shared and untouched, and COSMIC
     never writes GNOME's dconf database;
   - offer "Log out now" as part of the same action;
   - switching back sets the previous session; removal also purges the 26
     packages, the source, keyring, and pin, and deletes COSMIC's per-person
     settings unless "Also delete my settings and history" is unticked
     (medium);
   - a VM check on Zorin OS 18 and Ubuntu 24.04 of install, login through
     GDM, switching back, and removal (medium).
5. **Ticket 160** (the bar on COSMIC) and a COSMIC taskbar row in ticket 142,
   each medium; COSMIC's panel hidden while they run (decision C7).
6. **Not offered on Ubuntu 22.04.**

### C. COSMIC the primary desktop

Everything in B, plus: the gesture pipeline on COSMIC (large, upstream or the
evdev route), per-application window attention (large, upstream), Better OS's
taskbar and top bar replacing COSMIC's panel and dock by default (medium
each), deciding what happens to the GNOME code for people who stay on GNOME,
and for ticket 76's Debian distribution building and signing COSMIC itself
(Debian has none).

### D. A Better OS session on `cosmic-comp`, beside GNOME

1. **The session** (medium): session file, launcher (`cosmic-comp` running
   it), systemd user target starting COSMIC's settings daemon, portal,
   `cosmic-osd`, `cosmic-idle`, the locker, and `cosmic-bg`, Fcitx 5, and
   Better OS's own processes; `XDG_CURRENT_DESKTOP` naming Better OS and
   COSMIC so the portal and COSMIC's daemons apply.
2. **The install component** (medium): as B's item 4, with the 14-package
   closure instead of 26, the session package, and the same removal.
3. **The surfaces, all GPUI on layer-shell, no shell code:** the top bar
   (tickets 152, 156, 157, 160: its service and view as planned, opened as a
   layer surface), the taskbar or dock (ticket 142 item 3, medium), the
   notification island and center with Better OS's own
   `org.freedesktop.Notifications` (ticket 161, medium), the desktop and
   stacks in Better Files (ticket 144, medium), Better Launcher as an overlay
   layer surface (small), and a Better OS overview (large: window thumbnails
   through `ext-image-copy-capture`, workspaces through `ext_workspace_v1`).
4. **Status-area agents:** a NetworkManager secret agent and a BlueZ pairing
   agent in the top bar's status area, or COSMIC Settings for those prompts
   (medium).
5. **The COSMIC backends from B** that still apply in the session: the
   cosmic-config adapter, the Touchpad page, scroll factor, launcher key,
   server-side frame, Awake's ScreenSaver and fullscreen providers (as B,
   items 2 and 3).
6. **Gestures** (decision C8): the four-finger workspace switch works as
   released; Better OS's own transitions need the gesture patch (medium to
   write, then a rebase and rebuild with every COSMIC release), and the edge
   swipe the evdev route (ticket 161 (a), large with its security review).
7. **Not offered on Ubuntu 22.04**; the GNOME session stays everyone's
   fallback and the default until the Better OS session has passed its checks.

### What each option does to the tickets

| Ticket | A | B | C | D |
| --- | --- | --- | --- | --- |
| 142 (theme, corners, taskbar) | Item 2 needs a shell ADR, item 3 ADR 0036's row | GNOME as written; on COSMIC corners are the compositor's and the taskbar a layer surface | COSMIC part only | Corners already done by `cosmic-comp`; the taskbar a layer surface; the GNOME items wait |
| 144 (desktop stacks) | Waits on a desktop row in ADR 0036 | COSMIC first, GNOME after the row | Better Files on COSMIC | Better Files on a layer surface |
| 94 (Super+Space, file search) | As written | Also COSMIC's `system_actions` | Same as B | The session's own key bindings in `cosmic-comp`'s configuration; Tracker started by the session |
| 99 (Better Input) | IBus and Fcitx 5 | Fcitx 5 on COSMIC | Fcitx 5 first | Fcitx 5 in the session |
| 139, 140 (GDM) | As written | As written while GDM stays | As written while GDM stays | As written while GDM stays |
| 76 direction 3 (Debian) | GNOME is in Debian | As A | Better OS builds COSMIC | Better OS builds the 14 packages, or the fork, for Debian |

### Which of tickets 150 to 161 should wait

The owner decided on 2026-11-21 (`design/top-bar-spec.md` section 0) that the
bar is shape A, ADR 0036 is accepted, the bar carries no application menus
(tickets 153 to 155 are being revised and are not built as written in any
option), and XEmbed icons are handled as section 0 records. This list does not
change those decisions; it says which work each option would not use.

| Ticket | A | B | C | D |
| --- | --- | --- | --- | --- |
| 150 one dbusmenu crate | build | build | build | build |
| 151 shell adapter panel relay | build | build | wait | **wait** |
| 152 top bar component (service and view) | build | build | build | build, with the layer-shell path first |
| 153–155 menus | as revised by the owner's D5 | same | same | same |
| 156 tray | build | build | build | build |
| 157 status area | build | build | build | build, with the network and Bluetooth agents |
| 158 clock and notifications | build after 161's design | same | same | build after 161, notifications from Better OS's own daemon |
| 159 with Zorin's taskbar | build | build | wait | **wait** |
| 160 top bar on COSMIC | withdrawn | build | build | build (the main path) |
| 161 island and center | design, then GNOME build | design, both builds | design, COSMIC build | design; **its GNOME build (the ADR 0036 amendment for more docks, banner suppression) waits**; the edge swipe (evdev) is built in every option |

- **Wasted by building now if D is chosen:** ticket 151's JavaScript relay and
  its tests, ticket 159's Zorin taskbar entry, and the GNOME half of 161 (an
  ADR 0036 amendment, two more dock windows, banner suppression). The service
  and the view of 152, the tray of 156, the status area of 157, and 161's
  design all carry over, because the view opens as a layer surface instead of
  a dock.
- **Lost by waiting if D is not chosen in the end:** the owner's own GNOME
  desktop has no Better OS top bar, island, or center until 151 and 161's
  GNOME half are built after all, a delay of about the time those tickets
  take; nothing built in the meantime is thrown away.

## Risks

- **Runtime compatibility on Zorin's base is unproved.** The packages declare
  nothing newer than noble has, but were built against Pop's newer Wayland,
  Mesa, and PipeWire; a missing symbol shows only when it runs. This applies
  to B, C, and D.
- **A third-party repository on the person's machine.** Its key signs
  System76's kernel and systemd too; the pin is what keeps those off the
  machine, and a pin written wrong replaces the base system on the next
  upgrade. D can remove this risk by building the 14 packages itself.
- **COSMIC moves fast.** Twelve releases in eleven months and 503 compositor
  commits; a behaviour Better OS relies on (the decoration default changed on
  2026-11-06) can change without notice. In D this is the compositor under
  the whole session.
- **A fork Better OS cannot give back.** Upstream refuses machine-written
  code, so the gesture patch stays Better OS's to rebase for as long as
  upstream has no equivalent.
- **A session is a desktop's worth of small duties** (agents, prompts, OSD,
  locking, startup order), each of which GNOME does without being asked; a
  missed one shows up as a dialog that never appears.
- **Every desktop-facing test doubles** while GNOME stays supported, and the
  evidence here is one VM with software rendering, a live session, no GDM,
  and no layer-shell Better OS window.

## Recommendation

**D, beside GNOME**, in this order: build now only what carries over (150,
152 with its layer-shell path, 156, 157, 161's design and its edge swipe
through the evdev route, the cosmic-config adapter, Better Monitor's grouping
fix); hold 151, 159, and 161's GNOME build; then the Better OS session on
the released `cosmic-comp` with the top bar, notifications, desktop, and
launcher as layer surfaces; then its one-click component after a VM check on
Zorin OS 18 and Ubuntu 24.04; then decision C8's gesture patch if upstream
still has none. GNOME stays installed and the default until the Better OS
session has passed that check and a week on the owner's machine. Reasons:

- Everything the owner has asked for since this ticket was written (top bar,
  taskbar, island, center, one radius, desktop stacks, transitions that follow
  the fingers) is either already done by `cosmic-comp` or is a Better OS
  process on layer-shell there, while on GNOME each one is shell JavaScript
  and a further widening of ADR 0018, and the gesture requirement means
  driving GNOME's private transition methods per frame from outside.
- Stock COSMIC (B, C) does not meet the gesture requirement at all, and puts
  System76's panel, notifications, and desktop where Better OS's would be; D
  uses COSMIC's compositor and daemons and leaves the surfaces to Better OS.
- Only one piece needs a change inside the compositor (handing swipes to
  Better OS), and it can wait until the rest of the session works; the
  compositor is never written from nothing.
- Every Better OS window already runs on `cosmic-comp`, the services and the
  bar's service and view carry over, and what stays behind is the shell
  adapter, the GNOME keys, and the GTK frame.
- What D costs the owner today is real and is why GNOME stays beside it: the
  GNOME application grid, Zorin's extensions and Zorin Appearance, IBus
  (Fcitx 5 instead), Ubuntu 22.04, and an overview Better OS has to build.
  Choosing D also means accepting C2 (or building the 14 packages) and
  living with a compositor that moves weekly.

If the owner does not want a session of Better OS's own, the recommendation
is **A**: building B's second, stock desktop costs most of D's install work
and gives none of the requested transitions.

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
| `pop-noble-repo-shadows-host-packages.tsv`, `cosmic-closure-on-host-noble.txt`, `session-closure-on-host-noble.txt` | The 566 packages Pop's `noble` would replace on this host, the 26-package pinned closure for COSMIC, and the 14-package closure for option D's session |
| `scripts/` | The VM launcher, the QMP and serial driver, the closure script, and the screenshot cropper |

The fourth option's evidence is source only, read on 2026-11-21:
`cosmic-comp` master `6968c5d` (`src/input/mod.rs`, `src/state.rs`,
`src/lib.rs`, `src/main.rs`, `src/hooks.rs`, `src/shell/element/window.rs`),
`cosmic-session` (`src/main.rs`, `data/start-cosmic`, `data/dconf/profile/cosmic`),
`cosmic-osd`, `cosmic-idle`, COSMIC's pull request template, issues and pull
requests named above, System76's roadmap post, libinput's gesture
documentation, GNOME Shell 45.0 to 49.0's `overview.js`,
`workspaceAnimation.js`, and `swipeTracker.js`, and GPUI at `ae394f3`.

Not run: a light-mode pass; COSMIC or the Better OS session installed onto
Zorin OS 18 or Ubuntu 24.04 with the pin; GDM starting either session;
switching back and removing; a Better OS window as a layer-shell surface;
`cosmic-comp` with a gesture patch; real hardware; any touchpad.
