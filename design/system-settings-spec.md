# System settings in Better Manager

Which settings a person needs in the Better OS session, who holds each one,
what COSMIC Settings and GNOME Settings do with it today, which of them
become pages of Better Manager's control center (ADR 0017), how those pages
are built so that the same page serves GNOME and the session, in which order
they are built, and, for the pages proposed first, every state and the exact
copy in both languages. It answers ticket 229 and decision C9 of
[ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md) ("COSMIC
Settings inside the session at first; moving each page into Better Manager is
later work with its own tickets"). The boundary for privileged writes it
relies on is [ADR 0042](/engineering/decisions/0042-settings-pages-call-system-services.md)
(proposed). The work is cut into tickets 231 to 252. The look follows
`DESIGN.md`; every control is a `better_ui::fluent` control and every icon
comes from `better_ui::icon_set` (ticket 147). The mockups in
`design/system-settings/` draw the new System landing (`system`), Network and
internet (`network`), Wi-Fi (`wifi`), Bluetooth and devices (`bluetooth`),
Sound (`sound`), Displays (`displays`), Power and battery (`power`), Date and
time (`datetime`), and Notifications (`notifications`), each as `-light.html`
and `-dark.html`, in zh-TW, in the control center's own mockup technique
(`design/control-center/`). The English copy is in the tables below. The
rendered images are in `/home/tim/Developer/better-os-evidence/229/`.

Where this file and a ticket differ, the ticket decides behaviour and this file
decides layout and copy. `design/control-center-spec.md` stays the source for
every page it describes; this file adds pages and moves some cards, and says
so where it does.

## 0. Decisions that are the owner's

Nothing below is built until M1 to M3 are answered; each other decision blocks
only the tickets named with it.

| # | Decision | Recommendation | What each choice costs |
| --- | --- | --- | --- |
| M1 | **How far Better Manager replaces the desktops' settings apps.** (a) Every area in section 2 gets a Better Manager page in the end, built in two waves, and COSMIC Settings stops being something the person sees (M7). (b) Only the areas nothing serves in the session plus the daily ones (wave 1); COSMIC Settings stays for the rest. (c) Only the areas nothing serves in the session (Notifications, and the GNOME-only gaps of section 1, F2). | **(a), in the two waves of section 15.** | **(a):** one control center, as ticket 76's direction 6 asks, with one language, one appearance, and one search for every setting; 22 tickets, about 6 large, 9 medium, 7 small; every desktop-facing test doubles while GNOME stays supported. **(b):** about half of that; the person keeps two settings apps in the session, and COSMIC Settings keeps showing Panel, Dock, Workspaces, and Super-key choices for programs the session does not run (F3). **(c):** the least work; the person meets COSMIC Settings for every daily setting, with F3's pages in it, and the bar's settings links keep opening it. |
| M2 | **Which pages are needed before the session becomes the default** (`engineering/specs/better-os-session.md`, "Ready to become the default"). Recommended: an eighth item, "the wave 1 pages of `design/system-settings-spec.md` are built and pass their VM check", which are Network and internet, Bluetooth and devices, Sound, Displays, Power and battery, Date and time, and Notifications. | **Add the eighth item with wave 1.** | **Add it:** the session's default user meets no COSMIC Settings for the settings they change weekly, and the bar's links (ticket 160) open Better Manager; wave 1 is 3 large, 3 medium, and 1 small ticket on top of tickets 200 to 228. **Leave the bar as it is:** the session can become the default sooner, with C9's COSMIC Settings for all of it and F3's misleading pages in front of a new user. **Add Printers to wave 1 as well:** a person whose printer needs setting up has no settings program at all in the session (F1, F2); one more large ticket. The owner's machine has no printer (`lpstat -p`: "No destinations added"). |
| M3 | **The navigation pane** (section 4). (a) Windows 11's categories: 首頁, 系統, 藍牙與裝置, 網路和網際網路, 個人化, 應用程式, 帳戶, 時間與語言, 協助工具, 隱私權與安全性, 保持清醒, 修正, and 設定 in the footer, with Touchpad under Bluetooth and devices, Window attention inside Notifications, Default apps inside Apps, and Better Input's 輸入法 under Time and language. (b) Keep today's nine items and append one item per new page. | **(a).** | **(a):** a pane of twelve items however many pages follow, where a person who knows Windows 11 Settings finds things; Touchpad, Window attention, and Default apps move one level down (their `--page` names keep working), Better Input's proposed own item (`design/better-input-spec.md` section 7) becomes a sub-page; ticket 231 is medium. **(b):** nothing moves, but the pane grows to about 25 items and no longer fits the 960 × 600 minimum window. |
| M4 | **Under GNOME, an area whose setting GNOME keeps in its own store** (Displays, Night light, the screen and sleep times, the clock format, Notifications): (a) the Better Manager page with a GNOME backend; (b) the Better Manager page shows a link card that opens GNOME Settings' panel. | **(a) for wave 1**, (b) for wave 2 until each page is built. | **(a):** one page in both desktops, as GNOME stays the default; each wave 1 page has two backends and two sets of tests. **(b):** half the backends; under GNOME the person uses GNOME Settings for those areas and Better Manager for the rest, which is today's state. |
| M5 | **Night light in the session.** `cosmic-comp` 1.11 has no gamma or colour-temperature protocol (F4), so no client can tint the screen. (a) Add `wlr-gamma-control-unstable-v1` to Better OS's compositor patch (ticket 223), which widens ADR 0039's one-protocol patch and needs a new ADR. (b) Wait for upstream (COSMIC Settings keeps a night light page commented out, so upstream intends one); the card says it is not available yet. (c) Drop night light from the session. | **(b)**, checked at each Epoch release by ticket 226's routine. | **(a):** night light in the session soon; a second protocol to rebase each release, and a Better OS night-light service (a Rust client that sets the gamma ramps on a schedule). **(b):** no night light in the session until upstream ships it; the owner's own night light is off (`night-light-enabled false`). **(c):** saves nothing over (b). |
| M6 | **How a settings page changes a machine-wide setting** (time zone, the person's own password, other accounts, the device name, a system-wide network, printers). (a) The page's model calls the system's own service (timedated, AccountsService, hostnamed, NetworkManager, cups-pk-helper), whose polkit action asks for an administrator; `manager-daemon` gets a method only for a change no such service offers (the lid and the power button). (b) Every such change goes through a new `manager-daemon` method. | **(a), as ADR 0042 (proposed).** | **(a):** the services' own authorization stays in force; the daemon's reviewed surface grows by one interface (ticket 247) and AGENTS.md's privilege rule gains one sentence on acceptance. **(b):** a method and an action per setting; the daemon runs as root, so the service's own polkit check would always pass and the daemon would have to re-implement each one. |
| M7 | **COSMIC Settings in the session once wave 1 is built.** (a) Its main entry stays in Better Launcher. (b) The session hides its main entry (the session package ships `com.system76.CosmicSettings.desktop` with `NoDisplay=true` in its own `XDG_DATA_DIRS` directory, which comes first), and Better Manager's link cards still open its pages for the wave 2 areas. (c) Once every area has a page, `cosmic-settings` leaves the session's package closure. | **(b) after wave 1, then (c) after wave 2.** | **(a):** two settings apps in the launcher. **(b):** one; a person who knew COSMIC Settings reaches its pages only through Better Manager's link cards; ticket 252 is small. **(c):** one package fewer from System76's source; the closure changes, which C2 and ticket 204 compile into the daemon, so it is a daemon release. Whether anything in the closure needs `cosmic-settings` is unverified (ticket 252 reads it). |
| M8 | **Online accounts.** (a) Not offered in the session; under GNOME a link card opens GNOME Settings' panel. (b) A Better Manager page over GNOME Online Accounts (`goa-daemon`), whose sign-in flows are GNOME's own web views. | **(a).** | **(a):** no online accounts in the session, said on the card; nothing to build beyond the link card. **(b):** the sign-in flows need GNOME's OAuth handler and a web view Better OS does not have; large. |
| M9 | **The lid and the power button** (ticket 247). They are logind's (`HandleLidSwitch`, `HandlePowerKey` in `/etc/systemd/logind.conf`), which only root can write. (a) A `manager-daemon` method that writes one drop-in under `/etc/systemd/logind.conf.d/` from closed choices. (b) Not offered. | **(a), in wave 2.** | **(a):** a new privileged method and polkit action, under ADR 0017's rule (closed arguments, read back); whether logind 255 rereads its configuration without a restart is unverified, and ticket 247's first step. **(b):** the person edits a file as root, which AGENTS.md says makes the feature not done. |

## Sources this design was checked against

Read on 2026-11-21, nothing written:

- **COSMIC Settings 1.11.0**: `pop-os/cosmic-settings`, tag `epoch-1.11.0`,
  commit `f1d00fc06e18203e880771617657067c10c43321` (2026-10-20), cloned from
  `https://github.com/pop-os/cosmic-settings`. Page order `cosmic-settings/src/app.rs`
  217–234; features `cosmic-settings/Cargo.toml` (`[features]`); every page
  under `cosmic-settings/src/pages/`; English strings
  `i18n/en/cosmic_settings.ftl`; desktop entries `resources/applications/`;
  subcommands `cosmic-settings/src/main.rs` 64–162. Line numbers below are
  from that commit.
- **COSMIC's compositor 1.11.0**: `pop-os/cosmic-comp`, tag `epoch-1.11.0`,
  commit `8cb1802caada438bb5cc7670dfabdaca5fad0502`
  (`https://github.com/pop-os/cosmic-comp`): `cosmic-comp-config/src/lib.rs`
  80–115, `src/config/mod.rs` 362, the protocol handlers in
  `src/wayland/handlers/`.
- **COSMIC's settings daemon 1.11.0**: `pop-os/cosmic-settings-daemon`, tag
  `epoch-1.11.0`, commit `655b14fb9572dd7f7231ec6c343c63984e99741a`
  (`https://github.com/pop-os/cosmic-settings-daemon`): `src/main.rs` 50–51,
  174–248, `varlink-server/src/lib.rs` 8–31, `audio-client/README.md`,
  `data/polkit-1/rules.d/`.
- **`cosmic-randr` 1.11.0**: `pop-os/cosmic-randr`, tag `epoch-1.11.0`,
  commit `f1a97655c97e8799bf4a37a8f5103a8ab5e5068d`: `lib/src/output_manager.rs`
  7–13.
- **GNOME Settings 46.7**, the version Zorin ships
  (`gnome-control-center 1:46.7-0ubuntu0.24.04.6+zorin1`):
  `https://gitlab.gnome.org/GNOME/gnome-control-center/-/raw/46.7/` files
  `panels/display/cc-night-light-page.c`, `panels/privacy/cc-privacy-panel.c`,
  `cc-camera-page.c`, `cc-microphone-page.c`, `cc-location-page.c`,
  `cc-screen-page.c`.
- The owner's machine, read only: `dpkg-query -W` for the packages named
  below; the panels GNOME Settings ships (`/usr/share/applications/gnome-*-panel.desktop`:
  about, applications, background, bluetooth, color, datetime, display,
  keyboard, mouse, multitasking, network, notifications, online-accounts,
  power, printers, privacy, region, search, sharing, sound, system,
  universal-access, users, wacom, wifi, wwan); `strings /usr/bin/gnome-control-center`;
  the polkit defaults in `/usr/share/polkit-1/actions/` and the rules in
  `/usr/share/polkit-1/rules.d/`; `/usr/share/dbus-1/system.d/bluetooth.conf`;
  `busctl introspect org.freedesktop.Accounts` on the person's user object;
  `gdbus call … org.gnome.Mutter.DisplayConfig.GetCurrentState`;
  `timedatectl show`; `nmcli -t -f DEVICE,TYPE,STATE dev`; `lpstat -p`; and
  `gsettings get` of the keys quoted below.
- Better Manager as it is on `main` at `209e4c2`: `crates/manager-gui/src/navigation.rs`
  20–62, `shell.rs` 26–40, `deep_link.rs` 25–35, `pages/system/`,
  `crates/hardware-platform/src/bus.rs` 37–51, `crates/better-core/src/desktop.rs`.
- Not run: COSMIC Settings was not opened in a VM for this design (no VM image
  is on the machine; ticket 201 boots one). Every "COSMIC Settings does" below
  is read from its source, and every "in the session" behaviour is
  **unverified** until ticket 201's VM and each page's own VM check.

## 1. What shapes the design

- **F1. GNOME Settings does not run in the session.** `/usr/bin/gnome-control-center`
  carries "Running gnome-control-center is only supported under GNOME and
  Unity, exiting", and its desktop entries are `OnlyShowIn=GNOME;Unity;`
  (`gnome-printers-panel.desktop`, `gnome-online-accounts-panel.desktop`,
  `org.gnome.Settings.desktop`). So in the session, a setting COSMIC Settings
  lacks has no settings program at all.
- **F2. COSMIC Settings 1.11 lacks**: night light (the section is commented
  out, `pages/display/mod.rs` 215–224), printers, online accounts (the string
  `online-accounts` exists in the `.ftl`, no page uses it), notifications (the
  entry `com.system76.CosmicSettings.Notifications.desktop` runs
  `cosmic-settings notifications`, a subcommand `main.rs` 64–162 does not
  define), privacy (location, camera, microphone), screen lock times, removable
  media, a Wi-Fi hotspot (no `hotspot` anywhere in `pages/networking/`), the
  lid, and the power button.
- **F3. COSMIC Settings shows pages for programs the session does not run.**
  Desktop › Panel and Dock are inserted unconditionally under the `wayland`
  feature (`pages/desktop/mod.rs` 38–54), but `cosmic-panel` is not in the
  session's 14-package closure; Window management's Super key action offers
  Launcher, Workspaces, and Applications (`window_management.rs` 111–121),
  which rebind Super to COSMIC's `system_actions` for `cosmic-launcher`,
  `cosmic-workspaces`, and `cosmic-app-library`, none of them installed;
  Appearance's frosted glass names the panel, dock, launcher, and app library.
  Unless ticket 210 maps those `system_actions` to Better OS's programs, a
  person who changes the Super key there loses Better Launcher's binding.
- **F4. No night light is possible in the session today.** `cosmic-comp` 1.11
  has no gamma-control or colour-temperature protocol among its handlers
  (`src/wayland/handlers/`: no `gamma` file, and `grep -ri "gamma_control\|night.light"`
  in `src/` finds nothing). M5.
- **F5. The session already has the system services; only the user-facing
  stores differ.** NetworkManager, BlueZ, PipeWire, UPower,
  power-profiles-daemon, logind, timedated, localed, hostnamed,
  AccountsService, CUPS, geoclue, and the portal's permission store run in both
  sessions. What differs is the person's own configuration: GNOME's dconf keys
  under GNOME, `cosmic-config` files under `~/.config/cosmic/` in the session,
  and the compositor's display state (Mutter's `org.gnome.Mutter.DisplayConfig`
  under GNOME, the `zwlr_output_manager_v1` protocol in the session).
- **F6. Better Manager's System page cannot set brightness in the session.**
  Its brightness card reads and writes `org.gnome.SettingsDaemon.Power.Screen`
  (`crates/hardware-platform/src/bus.rs` 37–40), which only GNOME's
  `gsd-power` serves. In the session `cosmic-settings-daemon` serves
  `DisplayBrightness` on `com.system76.CosmicSettingsDaemon`
  (`/com/system76/CosmicSettingsDaemon`, `src/main.rs` 50–51, 176–216), which
  writes through logind's `Session.SetBrightness`; its keyboard brightness
  setter is empty (`src/main.rs` 222), so the keyboard backlight keeps
  UPower's `KbdBacklight`, which the card already falls back to. Ticket 235
  gives the brightness card its session backend.
- **F7. The top bar already reads half of these services.** Ticket 157 reads
  NetworkManager, the PulseAudio protocol of `pipewire-pulse`, and UPower for
  its flyouts; ticket 217 puts the NetworkManager secret agent and the BlueZ
  agent in `top-bar-service`. A page and a flyout that each kept their own
  client would be AGENTS.md's "a rule two places need" defect, so each
  service's model is one crate both use (section 3.1).

## 2. The inventory

### 2.1 Who holds each setting

"Session" is the Better OS session on `cosmic-comp`; "GNOME" is Zorin's and
Ubuntu's GNOME 46. A row with one entry is the same in both.

| Area | Who holds it in the session | Who holds it under GNOME | Interface Better OS uses |
| --- | --- | --- | --- |
| Displays: resolution, refresh, scale, position, orientation, mirroring | `cosmic-comp`, kept in `~/.local/state/cosmic-comp/outputs.ron` (`src/config/mod.rs` 362); clients change it through `zwlr_output_manager_v1` with COSMIC's `zcosmic_output_manager_v1` extension (`cosmic-randr`'s `lib/src/output_manager.rs` 7–13) | Mutter; `~/.config/monitors.xml` when the person changed something (absent on the owner's machine, which runs Mutter's own choice: 2880 × 1800 at 120 Hz, scale 2.0, offered scales 1.0, 2.0, 3.0, `GetCurrentState`) | session: a Rust Wayland client of those two protocols; GNOME: `org.gnome.Mutter.DisplayConfig` `GetCurrentState` and `ApplyMonitorsConfig` on the session bus |
| Brightness of the built-in display | `cosmic-settings-daemon` `DisplayBrightness`, through logind | `gsd-power` `org.gnome.SettingsDaemon.Power.Screen.Brightness` | as on the left, per desktop (F6) |
| Night light | nothing (F4) | `gsd-color`, keys `org.gnome.settings-daemon.plugins.color night-light-enabled`, `-schedule-automatic`, `-schedule-from`, `-schedule-to`, `-temperature` (`cc-night-light-page.c` 151–395) | GNOME: those keys through `defaults-platform`'s dconf writer |
| Appearance: dark or light, accent | `com.system76.CosmicTheme.Mode` `is_dark`, `com.system76.CosmicTheme.Dark`/`.Light` builders, read by every libcosmic program and synced to GTK by `cosmic-settings-daemon` | `org.gnome.desktop.interface color-scheme` (GNOME 46 has no accent key) | cosmic-config through ticket 206's writer; GNOME's key through the dconf writer |
| Wallpaper | `cosmic-bg`, `com.system76.CosmicBackground` | `org.gnome.desktop.background picture-uri`, `picture-uri-dark` | as on the left |
| Sound: devices, volumes, balance, per-application volume, profiles | PipeWire with WirePlumber (`pipewire 1.0.5`, `wireplumber 0.4.17`); COSMIC Settings goes through `cosmic-settings-daemon`'s varlink audio server (`$XDG_RUNTIME_DIR/com.system76.CosmicSettings`, `varlink-server/src/lib.rs` 27–31) | the same PipeWire | the PulseAudio protocol `pipewire-pulse` serves, as ticket 157 chose, in both |
| Over-amplification (above 100%) | `com.system76.CosmicAudio` `amplification_sink`, `amplification_source` (`pages/sound/mod.rs` 22–23) | `org.gnome.desktop.sound allow-volume-above-100-percent` | per desktop; the bar's slider reads the same key |
| Network: Wi-Fi, wired, VPN, airplane mode, hotspot | NetworkManager 1.46 on the system bus | the same | `org.freedesktop.NetworkManager` in both; airplane mode as below |
| Airplane mode | the kernel's rfkill; COSMIC reads `/dev/rfkill` (`subscriptions/airplane-mode/src/lib.rs` 13–53); `/dev/rfkill` carries an ACL for the active seat's user | `gsd-rfkill` (`org.gnome.SettingsDaemon.Rfkill` `AirplaneMode`) over the same rfkill | session: write `/dev/rfkill`; GNOME: `gsd-rfkill`'s property, so its own state agrees |
| Network secrets | Better OS's agent in the bar (ticket 217) | GNOME Shell's agent | none from the page: NetworkManager asks the agent |
| Bluetooth: power, pairing, devices | BlueZ 5.72; its bus policy lets any user talk to `org.bluez` (`bluetooth.conf`, `context="default"`) | the same | `org.bluez` in both; the page registers its own `org.bluez.Agent1` while it is open, as COSMIC Settings does (`subscriptions/bluetooth/src/agent.rs` 41) |
| Power mode, charge limit, battery | power-profiles-daemon 0.21, sysfs, UPower 1.90.3 | the same | Better Manager's System page today |
| Screen off and suspend when idle | `cosmic-idle`, `com.system76.CosmicIdle` `screen_off_time`, `suspend_on_ac_time`, `suspend_on_battery_time` (`pages/power/mod.rs` 23–40, 68, 357–374) | `org.gnome.desktop.session idle-delay` (600 s on the owner's machine), `org.gnome.settings-daemon.plugins.power sleep-inactive-ac-timeout`, `-ac-type`, `-battery-timeout`, `-battery-type` | per desktop |
| Lid, power button | logind, `/etc/systemd/logind.conf` (root) | logind, with `gsd-power` taking logind's power-key and lid inhibitors while GNOME runs | `manager-daemon` (M9) |
| Keyboard layouts and typing | `com.system76.CosmicComp` `xkb_config`, `keyboard_config` (`pages/input/keyboard/mod.rs` 380–608) and Fcitx 5's profile for input methods (C4) | `org.gnome.desktop.input-sources` and IBus | per desktop; input methods belong to Better Input (tickets 180–191, 221) |
| Keyboard shortcuts | `com.system76.CosmicSettings.Shortcuts`, read by `cosmic-comp` | `org.gnome.desktop.wm.keybindings`, `org.gnome.settings-daemon.plugins.media-keys`, `org.gnome.shell.keybindings` | per desktop |
| Mouse | `com.system76.CosmicComp` `input_default` (`pages/input/mod.rs` 64) | `org.gnome.desktop.peripherals.mouse` | per desktop |
| Touchpad | `com.system76.CosmicComp` `input_touchpad`, `input_devices` | `org.gnome.desktop.peripherals.touchpad` | Better Manager's Touchpad page (ticket 219 adds the session backend) |
| Date, time, time zone, automatic time | timedated; its polkit actions are `auth_admin_keep`, and Ubuntu's `com.ubuntu.desktop.rules` answers yes for an active local member of `sudo` | the same, plus automatic time zone (`org.gnome.desktop.datetime automatic-timezone`, `gsd-datetime` with geoclue) | `org.freedesktop.timedate1` in both |
| Clock format (24-hour, weekday, date, seconds, first day of week) | `com.system76.CosmicAppletTime` `military_time`, `show_weekday`, `show_seconds`, `show_date_in_top_panel`, `first_day_of_week` (`pages/time/date.rs` 56–271), which ticket 160's bar clock reads in the session | `org.gnome.desktop.interface clock-format` (`'12h'` on the owner's machine), `clock-show-weekday`, `clock-show-date`, `clock-show-seconds` | per desktop |
| Language and formats | COSMIC Settings writes the **system** locale through localed's `SetLocale` (`pages/time/region.rs` 843–864, `auth_admin_keep`) and the person's language through AccountsService `SetLanguage` (877–882) | the person's AccountsService `Language` and `FormatsLocale` (Ubuntu's AccountsService has `SetFormatsLocale`; the owner's reads `zh_TW.UTF-8`), `org.gnome.system.locale region` | AccountsService for the person in both; the session launcher (ticket 203) exports the formats (ticket 241) |
| Users, own password, login | AccountsService (`create_user`, `delete_user`, `set_account_type`, `set_password`, `set_icon_file`, `set_real_name`, `users/mod.rs` 536–755); `change-own-user-data` is `yes`, `change-own-password` and `user-administration` are `auth_admin` | the same | `org.freedesktop.Accounts` in both |
| Printers | CUPS 2.4.7; `cups-pk-helper` (`org.opensuse.cupspkhelper.mechanism.*`, `auth_admin_keep`, and yes for `sudo` and `lpadmin` under Ubuntu's rules); `gsd-printer` and `gsd-print-notifications` do not run in the session | the same, with GNOME's two printer daemons | IPP to CUPS for reading, cups-pk-helper for changes (ticket 246) |
| Accessibility | screen reader: `org.a11y.Status` `ScreenReaderEnabled` (`subscriptions/accessibility/src/lib.rs` 120); magnifier, colour filters, shake to find: `com.system76.CosmicComp` (`accessibility/mod.rs` 49, 479; `magnifier.rs` 50); high contrast through the COSMIC theme | `org.gnome.desktop.a11y.*`, `org.gnome.desktop.interface` (`text-scaling-factor`, `high-contrast` through the theme), the same `org.a11y.Status` | per desktop, the screen reader shared |
| Default applications | `~/.config/mimeapps.list` (`pages/applications/default_apps.rs` 122) | the same | `app-chooser-core` in both |
| Startup applications | `~/.config/autostart/` (`startup_apps.rs` 78, 249) | the same | in both |
| Notifications | Better OS's notification service (tickets 215, 216) | GNOME Shell, `org.gnome.desktop.notifications` (`show-banners`, `show-in-lock-screen`, and per application `org.gnome.desktop.notifications.application`) | session: the service's bus; GNOME: those keys |
| Location | geoclue 2.7; the portal's permission store, table `location` | `org.gnome.system.location enabled` (`false` on the owner's machine) and the same store (`cc-location-page.c` 28–29) | per desktop for the switch, the store in both |
| Camera, microphone | the portal's permission store, table `devices`, ids `camera`, `microphone` | the same, plus `org.gnome.desktop.privacy disable-camera`, `disable-microphone` (`cc-camera-page.c` 27–28, 427–449) | the store in both |
| Screen lock | the locker in `cosmic-greeter`; whether `cosmic-idle` locks at screen off and after what delay has no setting in COSMIC Settings 1.11 (**unverified**, ticket 201) | `org.gnome.desktop.screensaver lock-enabled` (`true`), `lock-delay`, `org.gnome.desktop.notifications show-in-lock-screen` (`cc-screen-page.c` 331–372) | per desktop |
| About, device name | hostnamed (`set-static-hostname` `auth_admin_keep`; yes for `sudo` through `gnome-control-center.rules`, a file that ships with GNOME Settings) | the same | `org.freedesktop.hostname1` |
| Removable media | no autorun program runs in the session | `org.gnome.desktop.media-handling` (`autorun-never` `true` on the owner's machine) | ticket 248 |
| Which drives mount | Better OS's block in `/etc/fstab` and udev rule (ticket 127, ADR 0032) | the same | Better Manager's System page today |
| Online accounts | none (`goa-daemon` is installed; no settings program offers it in the session) | GNOME Online Accounts | M8 |

### 2.2 What each settings program does today, and what Better Manager does

"COSMIC Settings" is what its 1.11 source offers on that page; "Page" is this
design's answer, with the ticket.

| Area | COSMIC Settings 1.11 | GNOME Settings 46 panel | Better Manager today | Page (wave, ticket, size) |
| --- | --- | --- | --- | --- |
| Displays | Displays: arrangement, per display enable, resolution, refresh, scale and additional scales, orientation, VRR, mirroring, a revert-after-countdown dialog; writes by running `cosmic-randr` (`pages/display/mod.rs` 1085) | Displays | brightness card only (System › Display) | **System › Displays**, wave 1, 235, large |
| Night light | none (commented out) | Displays › Night Light | none | inside Displays (GNOME); the session's card says it is not available (M5); 251 for the session, small once the compositor offers it |
| Appearance and wallpaper | Desktop › Appearance (mode, accent, frosted glass, density, fonts, icon theme, GTK export), Wallpaper (image, colour, slideshow, per display) | Appearance (style, background) | Settings › Appearance (Better OS windows only) | **Personalization**, wave 2, 242, medium |
| Panel, dock, workspaces, window management | Desktop › Panel, Dock, Window management, Workspaces | Multitasking | none | not offered: those are COSMIC's programs (F3); the session's own surfaces have their own settings in their tickets (152, 210–214) |
| Sound | Sound: output and input device, volume, balance, level, amplification, alert volume and sound, device profiles, per-application volume | Sound | none (the bar's flyout, ticket 157) | **System › Sound**, wave 1, 234, large |
| Network | Network and wireless: Wi-Fi (scan, connect, password dialog, forget, share by QR code), wired, VPN (import OpenVPN and WireGuard files, connect with username and password), airplane mode; other settings open `nm-connection-editor` (`pages/networking/mod.rs` 24, 385); its own NetworkManager secret agent while open (`backend.rs` 693) | Wi-Fi, Network, Mobile Network | none (the bar's flyout, 157; the agent, 217) | **Network and internet**, wave 1, 232, large |
| Bluetooth | Bluetooth: power, adapters, visible as the alias while open, nearby devices, pair with PIN confirmation, connect, disconnect, forget, rename, battery | Bluetooth | none (the agent, 217) | **Bluetooth and devices**, wave 1, 233, medium |
| Power | Power and battery: power mode (system76-power or power-profiles-daemon), battery and time left, connected devices' batteries, screen off after, suspend on AC, suspend on battery | Power | power mode, follow the charger, charge limit, battery, keyboard lighting, fan, sensors (System) | **System › Power and battery**, wave 1, 236, small |
| Lid and power button | none | none (GNOME 46 dropped the lid choice) | none | **inside Power and battery**, wave 2, 247, medium (M9) |
| Keyboard | Input › Keyboard: input sources (XKB layouts), special keys (compose, caps lock), repeat rate and delay, num lock at boot, shortcuts | Keyboard | keyboard backlight and colour (System) | **Bluetooth and devices › Keyboard**, wave 2, 239, large; input methods stay Better Input's (180–191, 221) |
| Mouse | Input › Mouse: primary button, speed, acceleration, natural scrolling, scroll speed | Mouse and Touchpad | none | **Bluetooth and devices › Mouse**, wave 2, 240, small |
| Touchpad | Input › Touchpad and gestures | Mouse and Touchpad | Touchpad page | moves under Bluetooth and devices (231); session backend 219 |
| Date and time | Time and language › Date and time: automatic time, time zone, 24-hour, seconds, weekday, first day of week, date in the panel; writes timedated (`date.rs` 163–300) and `CosmicAppletTime` | Date and Time | none | **Time and language › Date and time**, wave 1, 237, medium |
| Language and region | Time and language › Region and language: preferred languages (system locale and AccountsService), formats; "install additional languages" runs `gnome-language-selector` (`region.rs` 26, 267) | Region and Language | Settings › Language (Better OS windows only) | **Time and language › Language and region**, wave 2, 241, medium |
| Users | System and accounts › Users: add, remove, administrator, full name, picture, password | Users | none | **Accounts**, wave 2, 243, large |
| About | System › About: device name (hostnamed), hardware, operating system | About, System | System › About this PC (no device name) | wave 2, 250, small |
| Printers | none | Printers | none | **Bluetooth and devices › Printers**, wave 2, 246, large (M2 asks whether it moves to wave 1) |
| Accessibility | Accessibility: screen reader, magnifier (and its settings), high contrast, invert colours, colour filters, shake to find, mono audio, on-screen keyboard | Accessibility | none | **Accessibility**, wave 2, 244, large |
| Default and startup apps | Applications › Default applications, Startup applications, X11 compatibility | Apps › Default Apps; startup apps in Tweaks | Default apps (Better OS components only) | **Apps › Default apps** gains every default and startup apps, wave 2, 249, medium; X11 compatibility not offered (COSMIC's compositor settings for X11 scaling are the compositor's; ticket 249 records whether one of them is needed) |
| Notifications | none (F2) | Notifications | Window attention page | **System › Notifications**, wave 1, 238, medium; Window attention moves inside it |
| Privacy | none | Privacy and Security: screen lock, location, camera, microphone, file history, diagnostics | none | **Privacy and security**, wave 2, 245, medium |
| Removable media | none | Removable Media (in Apps › Default Apps in 46) | drive mount choices (System) | **System › Drives**, wave 2, 248, small |
| Online accounts | none | Online Accounts | none | a link card (M8) |

Online accounts aside, each area gets a page. Until its page is built, the
place in Better Manager where it will live holds a **link card** (section 5.2)
that opens COSMIC Settings' page in the session and GNOME Settings' panel under
GNOME, so search finds every setting from the first ticket on.

### 2.3 COSMIC Settings' page names and GNOME Settings' panels

For the link cards: Displays `displays` / `display`; Sound `sound` / `sound`;
Network `network`, `wireless`, `wired`, `vpn` / `network`, `wifi`; Bluetooth
`bluetooth` / `bluetooth`; Power `power` / `power`; Keyboard `keyboard` /
`keyboard`; Mouse `mouse` / `mouse`; Date and time `date-time` / `datetime`;
Language `region-language` / `region`; Users `users` / `users`; Appearance
`appearance`, `wallpaper` / `background`; Accessibility `accessibility` /
`universal-access`; Default apps `default-apps` / `applications`; Startup apps
`startup-apps` / none; Notifications none / `notifications`; Privacy none /
`privacy`; Printers none / `printers`; Online accounts none /
`online-accounts`; About `about` / `about` (from `resources/applications/*.desktop`
and `/usr/share/applications/gnome-*-panel.desktop`).

## 3. How a page is built

### 3.1 Crates

Every decision a page draws is made in a crate with no GPUI dependency, behind
a seam the tests replace with a fake service on a private bus, as
`hardware-core` and `hardware-platform` do. One crate per service, because the
bar's flyouts and the page share it (F7):

| Service | Model crate (decisions, states, copy keys) | Platform crate (the seam) | Used by |
| --- | --- | --- | --- |
| NetworkManager, rfkill | `network-core` | `network-platform` (zbus proxies, `/dev/rfkill`, `gsd-rfkill` under GNOME) | ticket 232, the bar's network part (157), the agent (217) |
| BlueZ | `bluetooth-core` (devices, pairing state machine, the agent's requests) | `bluetooth-platform` | 233, the agent (217) |
| PipeWire through the PulseAudio protocol | `sound-core` | `sound-platform` (the crate ticket 157 chooses) | 234, the bar's sound part (157) |
| The compositor's display state | `display-core` (modes, scales, arrangement rules, the revert countdown) | `display-platform` (Mutter's `DisplayConfig` and a `zwlr_output_manager_v1` client; `wayland-protocols-wlr` 0.3.12 is already in `Cargo.lock`) | 235 |
| timedated, the clock format | `datetime-core` | `datetime-platform` | 237, the bar's clock (160) |
| cosmic-config and dconf stores | — | `defaults-platform`'s cosmic-config adapter (ticket 206) and dconf writer (ticket 207), used as plain writers | 235–238 and wave 2 |

The names are a recommendation; the implementer may change them (the session
spec's "May decide"), not split one service across two crates. Whichever of a
page's ticket and ticket 157 starts first creates the service's crates to this
section; the other uses them.

### 3.2 Which backend a page uses

One rule, `better_core::desktop::CurrentDesktop`: `is_better_os_session()`
(ticket 200) chooses the session's store, `is_gnome()` GNOME's, and anything
else reads only the shared system services, drawing every per-desktop card
disabled with the sentence in section 5.1. No page reads `XDG_CURRENT_DESKTOP`
itself.

### 3.3 Who authorizes a write

Four kinds, from ADR 0042 (proposed, M6):

| Kind | Examples | How |
| --- | --- | --- |
| The person's own session | PipeWire, the compositor's outputs, cosmic-config and dconf in the home, Better OS's own services | the model crate writes; nothing asks |
| A system service whose polkit allows the active session | NetworkManager `network-control`, `enable-disable-wifi`, `settings.modify.own`, `wifi.share.*`; BlueZ; power-profiles-daemon; logind `SetBrightness`; AccountsService `change-own-user-data`; `/dev/rfkill` through the seat's ACL | the model crate calls the service |
| A system service whose polkit asks for an administrator | timedated, hostnamed, localed, NetworkManager `settings.modify.system`, AccountsService `change-own-password` and `user-administration`, cups-pk-helper | the model crate calls the service; the desktop's polkit agent asks (GNOME Shell, `cosmic-osd` in the session); Better OS never sees a password; a cancel is the "No permission" state |
| A machine-wide change no service offers | the lid and the power button | `manager-daemon`, one method and one polkit action per change, closed arguments, read back (ADR 0017's rule) |

### 3.4 Writing and reading back

Each write is followed by a read of the service's own state, and the card shows
what the service reports, never what was asked (the "A write read back
different" state). A store that another program changes (a key press, the bar,
COSMIC Settings, GNOME Settings) is watched, and the card follows it: D-Bus
`PropertiesChanged`, cosmic-config's file watch, dconf's change signal, the
Wayland protocol's events, the PulseAudio protocol's subscription.

### 3.5 Tests

Each model crate's tests run against fakes of its service on a private bus
(`dbus-run-session` under AGENTS.md's sandbox rule) or a fake Wayland
compositor, with every state and string in both languages; a page module's
tests check the cards it draws for each state. Each page ticket ends with a VM
check in both desktops on Zorin OS 18, screenshots under
`/home/tim/Developer/better-os-evidence/<ticket>/`.

## 4. Navigation

### 4.1 The pane (M3 (a))

| Item | zh-TW | English | Icon (`icon_set`) | Holds |
| --- | --- | --- | --- | --- |
| Home | 首頁 | Home | `dashboard` | unchanged |
| System | 系統 | System | `computer` | Displays, Sound, Notifications, Power and battery, Drives, Cooling, About |
| Bluetooth and devices | 藍牙與裝置 | Bluetooth & devices | `bluetooth` (new) | Bluetooth, Touchpad, Mouse, Keyboard, Printers |
| Network and internet | 網路和網際網路 | Network & internet | `network` | Wi-Fi, Ethernet, VPN, Mobile hotspot, Airplane mode |
| Personalization | 個人化 | Personalization | `palette` | Appearance, Wallpaper |
| Apps | 應用程式 | Apps | `apps` | Better OS components, Updates, Health, Activity, Default apps, Startup apps |
| Accounts | 帳戶 | Accounts | `user` | Your account, Other users, Sign-in options |
| Time and language | 時間與語言 | Time & language | `globe` | Date and time, Language and region, Input (Better Input) |
| Accessibility | 協助工具 | Accessibility | `accessibility` (new) | Vision, Hearing, Interaction |
| Privacy and security | 隱私權與安全性 | Privacy & security | `shield` | Screen lock, Location, Camera, Microphone |
| Keep awake | 保持清醒 | Keep awake | `keep-awake` | unchanged |
| Fixes | 修正 | Fixes | `fixes` | unchanged |
| Settings (footer) | 設定 | Settings | `settings` | unchanged; its Appearance and Language cards say they are Better Manager's own (section 13) |

Badges: Keep awake's dot and Apps' and Fixes' counts are unchanged; System
shows no badge. Every item exists from ticket 231 on; an area whose page is
not built yet holds its link card there.

Twelve items at 40px and the search box fill 572px under the 32px titlebar,
more than the 600px minimum window has with the footer; the pane's items
scroll above a fixed footer (section 16, kit gap).

### 4.2 Category landings

A category is a page of sub-page cards (`SettingsCard` with a chevron), as
Windows 11 Settings' System page, preceded by its live cards where it has
them. System's landing:

| Card | zh-TW header · description | English header · description | Opens |
| --- | --- | --- | --- |
| Displays | 顯示器 · 解析度、縮放、亮度、夜燈 | Displays · Resolution, scale, brightness, night light | System › Displays |
| Sound | 音效 · 輸出和輸入裝置、音量、應用程式音量 | Sound · Output and input devices, volume, app volume | System › Sound |
| Notifications | 通知 · 應用程式的通知、勿擾、視窗提醒 | Notifications · Alerts from apps, do not disturb, window attention | System › Notifications |
| Power and battery | 電源與電池 · 電源模式、充電上限、螢幕與睡眠 | Power & battery · Power mode, charge limit, screen and sleep | System › Power and battery |
| Drives | 磁碟機 · 登入時與插入時要掛載哪些磁碟機 | Drives · Which drives mount at sign-in and when plugged in | System › Drives (ticket 127's card, moved) |
| Cooling | 散熱 · 風扇模式、溫度 | Cooling · Fan mode, temperatures | System › Cooling (today's Cooling group) |
| About | 關於 · 裝置規格、裝置名稱 | About · Device specifications, device name | System › About (today's About this PC) |

Where today's System cards go: Power mode, follow the charger, charge limit,
battery → Power and battery; Brightness → Displays; Keyboard backlight and
Keyboard colour → Bluetooth and devices › Keyboard (until ticket 239 builds
that sub-page, a Keyboard sub-page with only those two cards); Fan mode,
Temperatures and fans → Cooling; Drives → Drives; Device specifications →
About. Each keeps its copy and behaviour from `design/control-center-spec.md`
section 4 and ticket 136.

### 4.3 Deep links

`better-manager --page <name>` keeps every name `deep_link.rs` 25–35 accepts
(`touchpad`, `window-attention`, `default-apps` now open the sub-pages they
moved to) and gains: `bluetooth-devices`, `network`, `network/wifi`,
`network/ethernet`, `network/vpn`, `network/hotspot`, `bluetooth`,
`personalization`, `accounts`, `time-language`, `accessibility`, `privacy`,
`system/displays`, `system/sound`, `system/notifications`, `system/power`,
`system/drives`, `system/cooling`, `system/about`, `time-language/date-time`,
`time-language/language`, `input` (Better Input's, unchanged), and one name per
wave 2 sub-page as its ticket adds it. The bar's settings links (tickets 157,
160) use these as each page lands, in both desktops.

### 4.4 Search

Every card of every new page is in the search index with its header,
description, page, and group, as section 1 of `design/control-center-spec.md`
says. A link card is indexed too, with the page it opens as its group:
searching 印表機 finds 印表機 › 藍牙與裝置 and opens the card. Added
synonyms (matched, never shown) for the words people type: Wi-Fi 網路 無線
wifi wlan; 藍牙 bluetooth 配對 耳機; 音量 聲音 喇叭 麥克風 sound volume
speaker microphone; 螢幕 顯示 解析度 縮放 display monitor resolution scale
hidpi; 夜燈 藍光 night light; 時區 時間 timezone clock ntp; 通知 勿擾 notification
dnd. The synonyms live in each page's copy table.

## 5. Shared patterns these pages add

### 5.1 States

The states of `design/control-center-spec.md` section 2 apply unchanged.
Added:

| State | How it is drawn | zh-TW | English |
| --- | --- | --- | --- |
| The service is not running (NetworkManager, BlueZ, PipeWire, timedated) | `InfoBar` Warning above the page, cards below disabled | {service} 沒有在執行，這一頁暫時無法讀取或變更。 | {service} isn't running, so this page can't read or change anything right now. |
| Service names in that sentence | — | 網路服務 · 藍牙服務 · 音效服務 · 時間服務 | The network service · The Bluetooth service · The sound service · The time service |
| This desktop cannot do it | the card drawn disabled, the sentence as its description | 這個桌面還不能調整這項設定。 | This desktop can't change this setting yet. |
| The session cannot do it yet (night light) | the card drawn disabled | Better OS 工作階段的視窗管理程式還不能調整螢幕色溫。 | The Better OS session's compositor can't change the screen's colour temperature yet. |
| An administrator is asked | Caption under the control, shown once it is opened | 變更時會要求輸入管理者密碼。 | You'll be asked for an administrator password. (from today's charge limit card) |
| A change will be undone unless kept | `ContentDialog` (5.3) | — | — |

### 5.2 The link card

A `SettingsCard` for an area whose page is not built yet, with the area's
object icon, its header and description from section 4, and at its trailing
edge the action icon `open-external` (already in `better-ui`'s set) and the program's name in Caption:

| Desktop | zh-TW | English | Runs |
| --- | --- | --- | --- |
| The session, COSMIC Settings has the page | 在 COSMIC 設定中開啟 | Opens in COSMIC Settings | `cosmic-settings <page>` with the names of section 2.3 |
| GNOME | 在 GNOME 設定中開啟 | Opens in GNOME Settings | `gnome-control-center <panel>` |
| Neither has it (session: printers, privacy, online accounts) | 在 Better OS 工作階段中還無法設定。請登入 GNOME 進行設定。 | This can't be set in the Better OS session yet. Sign in to GNOME to set it. | nothing; the card is not clickable |
| The program is not installed | 需要安裝 {program}。 | {program} needs to be installed. | nothing |

The program runs through the desktop's launch (`gtk-launch`-style
activation of its desktop entry), never through a shell. A link card is
replaced by the page in the ticket that builds the page.

### 5.3 Keep these settings?

For Displays, where a wrong mode can leave the screen black:

| | zh-TW | English |
| --- | --- | --- |
| Title | 要保留這些顯示設定嗎？ | Keep these display settings? |
| Body | {seconds} 秒後會還原成先前的設定。 | Your previous settings will come back in {seconds} seconds. |
| Primary | 保留變更 | Keep changes |
| Secondary | 還原 | Revert |

15 seconds; Escape and closing the window revert; the countdown runs in the
model with a test clock.

## 6. Network and internet

A pane item (`network`). Live cards first, then sub-pages.

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 網路和網際網路 | Network & internet |
| Status card (large, `SettingsCard` with the connection's icon) | {name} · 已連線 | {name} · Connected |
| Status lines | 已連線到網際網路 · 沒有網際網路 · 正在連線… · 未連線 | Connected to the internet · No internet · Connecting… · Not connected |
| Status button | 內容 | Properties |
| Wi-Fi card (toggle, chevron) | Wi-Fi · 顯示可用的網路 | Wi-Fi · Show available networks |
| Ethernet card (only with a wired device) | 乙太網路 · {state} | Ethernet · {state} |
| VPN card | VPN · {n} 個 VPN 連線 / 新增、連線、管理 | VPN · {n} VPN connections / Add, connect, manage |
| Mobile hotspot card (toggle, only with a Wi-Fi device that can share) | 行動熱點 · 讓其他裝置透過這台電腦上網 | Mobile hotspot · Share this PC's internet connection |
| Airplane mode card (toggle) | 飛航模式 · 關閉所有無線通訊 | Airplane mode · Turn off all wireless communication |
| Advanced (link card to `nm-connection-editor`) | 其他網路設定 · Proxy、802.1X、橋接與其他進階設定 | More network settings · Proxy, 802.1X, bridges, and other advanced settings |
| …its trailing text | 網路連線 | Network Connections |

Internet reachability is NetworkManager's `Connectivity` (`full`, `limited`,
`portal`, `none`, `unknown`); `portal` says 需要登入這個網路 / This network needs
you to sign in, with 開啟登入頁面 / Open sign-in page, which opens
NetworkManager's connectivity-check URL in the default browser.

### 6.1 Wi-Fi

| Element | zh-TW | English |
| --- | --- | --- |
| Title (breadcrumb) | 網路和網際網路 › Wi-Fi | Network & internet › Wi-Fi |
| Wi-Fi toggle card | Wi-Fi | Wi-Fi |
| Connected network (`SettingsExpander`) | {ssid} · 已連線，安全 | {ssid} · Connected, secured |
| …rows | 內容 · 自動連線（範圍內時） · 中斷連線 | Properties · Connect automatically when in range · Disconnect |
| Group | 可用的網路 | Available networks |
| Network row | {ssid}，訊號強度圖示，鎖頭代表需要密碼，已儲存的網路附上「已儲存」 | {ssid}, signal icon, a lock when secured, "Saved" for a known network |
| Row expanded, open network | 連線 · ☐ 自動連線 | Connect · ☐ Connect automatically |
| Row expanded, personal security (WEP, WPA, WPA2, WPA3 SAE) | 輸入網路安全性金鑰 [••••] 👁 · 下一步 · 取消 | Enter the network security key [••••] 👁 · Next · Cancel |
| Enterprise security (802.1X) | 連線後會出現登入對話框。 | A sign-in dialog will appear when you connect. |
| Connecting | 正在連線… | Connecting… |
| Wrong key (`NM_DEVICE_STATE_REASON_NO_SECRETS`, `SUPPLICANT_DISCONNECT`) | 無法連線。網路安全性金鑰可能不正確。 | Couldn't connect. The network security key might be wrong. |
| Other failure | 無法連線到「{ssid}」。 | Couldn't connect to "{ssid}". |
| Scanning (the list empty while a scan runs) | 正在尋找網路… | Looking for networks… |
| No networks | 找不到任何網路。 | No networks found. |
| Wi-Fi off | Wi-Fi 已關閉。 | Wi-Fi is off. |
| Airplane mode on | 飛航模式已開啟。關閉它才能使用 Wi-Fi。 / 關閉飛航模式 | Airplane mode is on. Turn it off to use Wi-Fi. / Turn off airplane mode |
| Command bar | 加入隱藏的網路 · 管理已知的網路 · 重新整理 | Add a hidden network · Manage known networks · Refresh |
| Hidden network dialog | 加入隱藏的網路 · 網路名稱 · 安全性類型（無、WPA/WPA2 個人、WPA3 個人）· 網路安全性金鑰 · 儲存 · 取消 | Add a hidden network · Network name · Security type (None, WPA/WPA2-Personal, WPA3-Personal) · Network security key · Save · Cancel |
| Known networks sub-page row | {ssid} · 忘記 | {ssid} · Forget |
| Forget dialog | 要忘記「{ssid}」嗎？之後要再連線時，需要重新輸入密碼。 · 忘記 · 取消 | Forget "{ssid}"? You'll need to enter its password again to connect. · Forget · Cancel |
| Share (in Properties) | 分享這個網路 · 用手機掃描 QR 碼即可連線 · 顯示 QR 碼 | Share this network · Scan the QR code with a phone to join · Show QR code |
| No Wi-Fi device | the Wi-Fi card and sub-page are not drawn | — |

The personal key is passed in the connection NetworkManager is asked to add
(`AddAndActivateConnection2` with `802-11-wireless-security.psk`), so no agent
is involved for it; enterprise networks and a saved network whose key changed
are asked by the agent (GNOME Shell under GNOME, ticket 217's under the
session). A connection the page adds is the person's own (`permissions`
`user:{name}`, `settings.modify.own`, no password asked); a system-wide
connection someone else made is changed with `settings.modify.system` and the
administrator note. The QR code needs the key, read with `GetSecrets`, which
NetworkManager authorizes for the connection's owner.

### 6.2 Properties (Wi-Fi and Ethernet)

| Row | zh-TW | English |
| --- | --- | --- |
| IP assignment | IP 指派：自動 (DHCP) / 手動 · 編輯 | IP assignment: Automatic (DHCP) / Manual · Edit |
| DNS assignment | DNS 伺服器指派：自動 / 手動 · 編輯 | DNS server assignment: Automatic / Manual · Edit |
| Facts (Copy button) | SSID · 通訊協定 · 安全性類型 · 頻帶 · IPv4 位址 · IPv6 位址 · DNS 伺服器 · 實體位址 (MAC) | SSID · Protocol · Security type · Network band · IPv4 address · IPv6 address · DNS servers · Physical address (MAC) |
| Metered | 計量付費連線 · 部分應用程式在這個網路上會減少使用數據 | Metered connection · Some apps reduce data use on this network |
| Edit IP dialog | 編輯 IP 設定 · 自動 (DHCP) / 手動 · IP 位址 · 子網路前置長度 · 閘道 · 儲存 · 取消 | Edit IP settings · Automatic (DHCP) / Manual · IP address · Subnet prefix length · Gateway · Save · Cancel |
| Invalid address | 這不是有效的 IP 位址。 | That isn't a valid IP address. |

Metered is NetworkManager's `connection.metered`.

### 6.3 Ethernet, VPN, hotspot

| Element | zh-TW | English |
| --- | --- | --- |
| Ethernet states | 已連線 · 纜線已拔除 · 正在連線… · 無法連線 | Connected · Cable unplugged · Connecting… · Couldn't connect |
| VPN group | VPN 連線 | VPN connections |
| VPN row | {name} · 已連線 / 未連線 · 連線 / 中斷連線 · … 移除 | {name} · Connected / Not connected · Connect / Disconnect · … Remove |
| Add VPN | 新增 VPN · 從檔案匯入（WireGuard .conf、OpenVPN .ovpn） | Add VPN · Import from a file (WireGuard .conf, OpenVPN .ovpn) |
| Import failed | 無法匯入這個檔案。{reason} | Couldn't import this file. {reason} |
| OpenVPN plugin missing | 需要安裝 OpenVPN 外掛（network-manager-openvpn）才能匯入 .ovpn 檔。 | The OpenVPN plug-in (network-manager-openvpn) needs to be installed to import .ovpn files. |
| Remove VPN dialog | 要移除「{name}」嗎？ · 移除 · 取消 | Remove "{name}"? · Remove · Cancel |
| No VPN | 還沒有 VPN 連線。 | No VPN connections yet. |
| Hotspot rows | 網路名稱 {ssid} · 密碼 {password} · 頻帶 2.4 GHz / 5 GHz · 編輯 · 已連線的裝置 {n} 台 | Network name {ssid} · Password {password} · Band 2.4 GHz / 5 GHz · Edit · {n} devices connected |
| Hotspot on one Wi-Fi card | 開啟行動熱點時，這台電腦的 Wi-Fi 會中斷。 | Turning on the hotspot disconnects this PC from Wi-Fi. |

The file chooser is the portal's (ticket 88). Importing runs NetworkManager's
own importer (`nmcli connection import type wireguard|openvpn file <path>`),
the one place its plug-ins parse a file, as COSMIC Settings does
(`pages/networking/vpn/mod.rs` 499, 1116); a VPN's secrets at connect time are
the agent's. The hotspot is a NetworkManager connection in `ap` mode with
`ipv4.method shared` (`wifi.share.protected`, allowed for the active session);
its default name is the device name, its password 12 random letters and
digits, kept in the connection.

## 7. Bluetooth and devices

A pane item (`bluetooth-devices`).

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 藍牙與裝置 | Bluetooth & devices |
| Bluetooth card (toggle) | 藍牙 · 可被偵測為「{alias}」 | Bluetooth · Discoverable as "{alias}" |
| …off | 藍牙 · 已關閉 | Bluetooth · Off |
| Add device (accent button on the card) | 新增裝置 | Add device |
| Group | 裝置 | Devices |
| Device card | {name} · 已連線 · 電池 {n}% / 已配對 | {name} · Connected · Battery {n}% / Paired |
| Device actions | 連線 / 中斷連線 · … 重新命名 · 移除裝置 | Connect / Disconnect · … Rename · Remove device |
| Remove dialog | 要移除「{name}」嗎？之後要再使用，需要重新配對。 · 移除 · 取消 | Remove "{name}"? You'll need to pair it again to use it. · Remove · Cancel |
| Rename dialog | 重新命名裝置 · 名稱 · 儲存 · 取消 | Rename device · Name · Save · Cancel |
| No device | 還沒有配對的裝置。 | No paired devices yet. |
| Sub-page cards (after the device list) | 觸控板 · 滑鼠 · 鍵盤 · 印表機 | Touchpad · Mouse · Keyboard · Printers |
| No adapter | 這台電腦沒有藍牙。 (the toggle, Add device, and device list are not drawn; the sub-page cards stay) | This PC doesn't have Bluetooth. |
| Blocked by airplane mode | 飛航模式已開啟，藍牙已關閉。 / 關閉飛航模式 | Airplane mode is on, so Bluetooth is off. / Turn off airplane mode |

**Add device dialog** (`ContentDialog`; discovery runs only while it is open):

| State | zh-TW | English |
| --- | --- | --- |
| Title | 新增裝置 | Add a device |
| Hint | 請確認裝置已開啟並進入配對模式。 | Make sure the device is on and ready to pair. |
| Searching | 正在搜尋… | Searching… |
| Row | {name}（裝置類型圖示） | {name} (with its device-kind icon) |
| Confirm a code (`RequestConfirmation`) | 「{name}」上顯示的是 {code} 嗎？ · 相符 · 不相符 | Does "{name}" show {code}? · It matches · It doesn't match |
| Type a code on the device (`DisplayPasskey`, `DisplayPinCode`) | 請在「{name}」上輸入 {code}，然後按 Enter。 | Type {code} on "{name}", then press Enter. |
| Enter a PIN (`RequestPinCode`, `RequestPasskey`) | 輸入「{name}」的 PIN 碼 · 連線 | Enter the PIN for "{name}" · Connect |
| Paired | 「{name}」已可以使用。 · 完成 | "{name}" is ready to go. · Done |
| Failed | 無法和「{name}」配對。請再試一次。 · 重試 | Couldn't pair with "{name}". Try again. · Retry |
| Nothing found after 30 s | 還沒有找到裝置。 | No devices found yet. |

While the page is open the adapter is discoverable and the page registers its
own agent (`RegisterAgent`, `RequestDefaultAgent` only while the dialog is
open), as GNOME Settings and COSMIC Settings do, and gives both back when it
closes; the agent's state machine is `bluetooth-core`'s, the same one ticket
217's bar agent runs. The battery is BlueZ's `Battery1`. Device kinds from
BlueZ's `Icon` property, drawn with object icons: headphones, headset,
keyboard, mouse, phone, computer, game controller, speaker, other.

## 8. Sound (System › Sound)

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 系統 › 音效 | System › Sound |
| Group | 輸出 | Output |
| Output device (`ComboBox`) | 選擇播放聲音的位置 · {port} - {card} | Choose where to play sound · {port} - {card} |
| Volume (slider, mute button at its leading edge) | 音量 | Volume |
| Balance (slider, stereo devices only) | 平衡 · 左 · 右 | Balance · Left · Right |
| Over-amplification (toggle) | 允許音量超過 100% · 可能會使聲音失真 | Allow volume above 100% · Sound may distort |
| Group | 輸入 | Input |
| Input device | 選擇要用來說話或錄音的裝置 · {port} - {card} | Choose a device for speaking or recording · {port} - {card} |
| Input volume | 輸入音量 | Input volume |
| Level meter | 一條隨聲音跳動的音量條 | a level bar that moves with the sound |
| Group | 應用程式音量 | App volume |
| App row | {application} · 音量 · 靜音 · 輸出裝置 ▾ | {application} · Volume · Mute · Output device ▾ |
| No app | 沒有應用程式正在播放聲音。 | No apps are playing sound. |
| Group | 進階 | Advanced |
| Device profiles (`SettingsExpander`) | 音效卡設定檔 · {card}：{profile} ▾ | Sound card profiles · {card}: {profile} ▾ |
| Bluetooth headset note | 使用耳機的麥克風時，音質會降低。 | Sound quality drops while the headset's microphone is in use. |
| Test (button on the output card) | 測試 | Test |
| No output | 找不到輸出裝置。 | No output devices found. |
| No input | 找不到輸入裝置。 | No input devices found. |
| Unplugged port | {port}（未插入） | {port} (unplugged) |
| No audio server | the service state of 5.1 with 音效服務 / The sound service | — |

Devices are PulseAudio sinks and sources with their ports; the default is
`set-default-sink`; an app is a sink input, moved with `move-sink-input`.
Test plays the sound theme's `audio-test-signal` through the default sink. The
level meter is a peak-detect stream on the source's monitor, opened only while
the page is visible. Over-amplification raises the slider's maximum to 150%,
and the bar's slider reads the same key per desktop. Alert volume is not
offered: whether `pipewire-pulse` 1.0.5 keeps a per-role volume for event
sounds is unverified, and ticket 234 adds it if it does.

## 9. Displays (System › Displays)

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 系統 › 顯示器 | System › Displays |
| Arrangement (two or more displays) | 拖曳顯示器來調整排列方式 · 識別 | Drag displays to rearrange them · Identify |
| Multiple displays (`ComboBox`) | 延伸這些顯示器 · 同步顯示這些顯示器 · 只在 {display} 上顯示 | Extend these displays · Duplicate these displays · Show only on {display} |
| Display selector (`SelectorBar`, two or more) | 1 · 2 … | 1 · 2 … |
| Display names | 內建顯示器 · {vendor} {model} | Built-in display · {vendor} {model} |
| Group | 亮度與色彩 | Brightness & colour |
| Brightness (built-in only) | today's card, from System | today's card |
| Night light (`SettingsExpander`, toggle) | 夜燈 · 使用較暖的色彩以減少藍光 | Night light · Use warmer colours to cut blue light |
| …rows | 排程：日落到日出 / 自訂時間 · 開啟 {time} · 關閉 {time} · 強度 (slider) | Schedule: Sunset to sunrise / Set hours · Turn on {time} · Turn off {time} · Strength (slider) |
| …in the session | the state of 5.1, "The session cannot do it yet" | — |
| Group | 縮放與版面配置 | Scale & layout |
| Scale (`ComboBox`) | 縮放 · 變更文字、應用程式與其他項目的大小 · {n}%（建議） | Scale · Change the size of text, apps, and other items · {n}% (Recommended) |
| GNOME without fractional scaling | 只提供整數倍縮放。開啟「分數縮放」可以使用 125%、150%、175%。 · 分數縮放 (toggle) | Only whole-number scales are offered. Turn on fractional scaling for 125%, 150%, and 175%. · Fractional scaling (toggle) |
| Resolution (`ComboBox`) | 顯示器解析度 · {w} × {h}（建議） | Display resolution · {w} × {h} (Recommended) |
| Orientation (`ComboBox`) | 顯示方向 · 橫向 · 直向 · 橫向（翻轉） · 直向（翻轉） | Display orientation · Landscape · Portrait · Landscape (flipped) · Portrait (flipped) |
| Refresh rate (`ComboBox`) | 更新頻率 · {n} Hz | Refresh rate · {n} Hz |
| Variable refresh rate (toggle, where offered) | 可變更新頻率 · 讓遊戲和影片更順暢 | Variable refresh rate · Smoother games and video |
| Make primary (only with two or more, where offered) | 設為主要顯示器 | Make this my main display |
| Turn off (external display, two or more) | 停用這個顯示器 | Turn off this display |
| One display left | 至少要有一個顯示器保持開啟。 | At least one display has to stay on. |
| Apply failed | 顯示器沒有接受這個設定，已保留先前的設定。 | The display didn't accept that, so your previous settings are kept. |

Every change that can blank a screen (resolution, refresh rate, orientation,
multiple displays, turning a display off, variable refresh) opens 5.3's
dialog; scale and brightness do not. The recommended resolution is the
display's preferred mode; the recommended scale is the compositor's own
choice for that display (Mutter's preferred scale under GNOME;
`cosmic-comp`'s default in the session, read when the display has no stored
configuration). Fractional scaling under GNOME is Mutter's experimental
feature `scale-monitor-framebuffer` in `org.gnome.mutter experimental-features`
(`[]` on the owner's machine, which is why GNOME offers 1.0, 2.0, 3.0 only),
as Ubuntu's own GNOME Settings switches it. Identify shows each display's
number on it: in the session a layer-shell surface per output (ticket 200's
surface), under GNOME `org.gnome.Shell.ShowMonitorLabels`. Variable refresh
and make primary are drawn only where the backend reports them; whether
`cosmic-comp` 1.11 has a primary output is unverified (ticket 235).

## 10. Power and battery (System › Power and battery)

Today's Power group (`design/control-center-spec.md` section 4), moved here
unchanged, then:

| Element | zh-TW | English |
| --- | --- | --- |
| Group | 螢幕與睡眠 | Screen & sleep |
| Screen off (`ComboBox`, one value; neither desktop has one per power source) | 在此時間後關閉螢幕 · 1 分鐘 · 2 分鐘 · 3 分鐘 · 5 分鐘 · 10 分鐘 · 15 分鐘 · 30 分鐘 · 永不 | Turn off my screen after · 1 minute … 30 minutes · Never |
| Sleep on battery | 使用電池時，在此時間後讓電腦睡眠 · 15 分鐘 · 20 分鐘 · 30 分鐘 · 45 分鐘 · 1 小時 · 2 小時 · 永不 | On battery power, put my device to sleep after · 15 minutes … 2 hours · Never |
| Sleep plugged in | 接上電源時，在此時間後讓電腦睡眠 | When plugged in, put my device to sleep after |
| Other value found | {time}（在其他地方設定的） | {time} (set elsewhere) |
| Keep awake link | 保持清醒 · 暫時不讓這台電腦睡眠 › | Keep awake · Keep this PC from sleeping for a while › |
| Group | 其他裝置的電量 | Other devices' batteries |
| Row (UPower devices other than the PC's battery) | {device} · {n}% | {device} · {n}% |
| None | the group is not drawn | — |

The battery card gains ticket 170's state, 已接上電源，維持在 {limit}% /
Plugged in, holding at {limit}%, in the same words module the bar uses. Lid and
power button (ticket 247) join this group in wave 2.

## 11. Date and time (Time and language › Date and time)

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 時間與語言 › 日期和時間 | Time & language › Date & time |
| Now (large line) | {time} · {date} | {time} · {date} |
| Set time automatically (toggle) | 自動設定時間 · 從網際網路同步 | Set time automatically · Sync from the internet |
| Set manually (button, only while automatic is off) | 手動設定日期和時間 · 變更 | Set the date and time manually · Change |
| Manual dialog | 變更日期和時間 · 日期 · 時間 · 變更 · 取消 | Change date and time · Date · Time · Change · Cancel |
| Time zone (`AutoSuggestBox` over the list) | 時區 · (UTC+08:00) 台北 | Time zone · (UTC+08:00) Taipei |
| Automatic time zone (toggle, GNOME) | 自動設定時區 · 需要開啟定位服務 | Set time zone automatically · Needs location services |
| …in the session | the state of 5.1, "This desktop can't change this setting yet" | — |
| Group | 時鐘 | Clock |
| 24-hour (toggle) | 24 小時制 | 24-hour time |
| Weekday, date, seconds (toggles) | 顯示星期 · 顯示日期 · 顯示秒數 | Show the day of the week · Show the date · Show seconds |
| First day of week (`ComboBox`, session) | 一週的第一天 · 星期日 · 星期一 · 星期六 | First day of the week · Sunday · Monday · Saturday |
| …under GNOME | 依照地區格式決定。 (disabled) | Set by your region format. (disabled) |
| No NTP service (`CanNTP` false) | 這台電腦沒有可用的時間同步服務。 | This PC has no time sync service. |
| Time zone search, nothing found | 找不到「{query}」。 | No match for "{query}". |

Time zone names come from timedated's `ListTimezones`, shown with their
current offset and the city in the person's language (from the time zone
database's own names; zh-TW city names for the zones Taiwan's users meet
first are in the copy table, the rest stay as the database writes them). Set
time, set time zone, and automatic time are timedated's methods with
`interactive = true`, and the administrator note; on Ubuntu an active member
of `sudo` is not asked (`com.ubuntu.desktop.rules`). The clock group writes
the store the bar reads (section 2.1).

## 12. Notifications (System › Notifications)

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 系統 › 通知 | System › Notifications |
| Do not disturb (toggle) | 勿擾 · 通知會直接放進通知中心，不會跳出 | Do not disturb · Notifications go straight to the notification center |
| Lock screen (toggle) | 在鎖定畫面顯示通知 | Show notifications on the lock screen |
| Group | 來自應用程式的通知 | Notifications from apps |
| App row (`SettingsExpander`, toggle) | {application} · 橫幅、音效 | {application} · Banners, sounds |
| …rows | 顯示通知橫幅 · 播放音效 · 在鎖定畫面顯示內容 | Show banners · Play a sound · Show content on the lock screen |
| No app yet | 還沒有應用程式傳送通知。 | No apps have sent notifications yet. |
| Group | 視窗提醒 | Window attention |
| (Window attention's cards, from ticket 106's page, moved) | today's copy | today's copy |
| Session, service not running | the service state of 5.1, 通知服務 / The notification service | — |

Under GNOME the switches are `org.gnome.desktop.notifications`'
`show-banners` (Do not disturb is its inverse), `show-in-lock-screen`, and per
application `enable`, `show-banners`, `enable-sound-alerts`,
`details-in-lock-screen`, written through the dconf writer (GNOME Shell reads
them live). In the session they are the settings of Better OS's notification
service (tickets 215, 216), read and written over its own bus, the service
being their only owner; the per-application list is the applications that
have sent a notification, as GNOME's is.

## 13. Wave 2 pages, in brief

Each ticket writes its page's section into this file first, with every state
and both languages, as wave 1's are, and its mockups.

| Page | What it holds | Session store | GNOME store |
| --- | --- | --- | --- |
| Keyboard (239) | layouts (add, order, remove), the switch key, compose and caps lock, repeat rate and delay, num lock at start, shortcuts (view, change, add a custom one, conflicts), today's backlight and colour cards; input methods open Better Input's page | `xkb_config`, `keyboard_config`, `com.system76.CosmicSettings.Shortcuts`; Fcitx 5's layouts kept in step (ticket 221) | input sources, `xkb-options`, keybindings |
| Mouse (240) | primary button, pointer speed, acceleration, scroll speed, natural scrolling | `input_default` | `org.gnome.desktop.peripherals.mouse` |
| Language and region (241) | display language order, formats, the note that it takes effect at next sign-in, install a language (runs `gnome-language-selector` from `language-selector-gnome 0.225` until a daemon method exists) | AccountsService `SetLanguages`, `SetFormatsLocale`; the session launcher exports `LANG` and `LC_*` from them (**unverified** that GDM does it for a non-GNOME session) | the same, and `org.gnome.system.locale region` |
| Personalization (242) | dark or light, accent colour, wallpaper (picture, colour, fit, per display) | `CosmicTheme.Mode`, theme builders, `CosmicBackground` | `color-scheme`, `background` keys |
| Accounts (243) | your name, picture, password; other users (add, remove, administrator); automatic sign-in | AccountsService | the same |
| Accessibility (244) | screen reader, text size, high contrast, magnifier, colour filters, reduce motion, mono audio, sticky and slow keys where the desktop has them | `org.a11y.Status`, `CosmicComp` `accessibility_zoom`, the a11y manager protocol | `org.gnome.desktop.a11y.*`, `org.gnome.desktop.interface` |
| Privacy and security (245) | screen lock (on, delay), location (on, per app), camera and microphone (per app), recent files | the portal's permission store; the locker's delay **unverified**; location's switch has no session store (geoclue's agent in the session: **unverified**) | `org.gnome.desktop.screensaver`, `org.gnome.system.location`, `org.gnome.desktop.privacy`, the store |
| Printers (246) | printers found and added, default, queue, add (driverless IPP Everywhere first), remove, test page | CUPS over IPP for reading, cups-pk-helper for changes | the same |
| Lid and power button (247) | when I close the lid (on battery, plugged in), the power button | `manager-daemon` drop-in under `/etc/systemd/logind.conf.d/` (M9) | the same for the lid; under GNOME the power button is `org.gnome.settings-daemon.plugins.power power-button-action` |
| Removable media (248) | what happens when a drive, phone, or disc is plugged in | no handler runs in the session; ticket 248 decides whether Better Files offers it | `org.gnome.desktop.media-handling` |
| Default and startup apps (249) | web browser, mail, file manager, music, video, photos, calendar, terminal, text editor, by file type; apps that start at sign-in | `mimeapps.list`, `~/.config/autostart/` | the same |
| About (250) | device name, hardware, operating system, copy | hostnamed | the same |

Settings (the footer page) keeps Appearance and Language as Better Manager's
own, with new descriptions so no one mistakes them for the desktop's:
Better Manager 的外觀。桌面的深淺色在「個人化」。 / How Better Manager looks.
The desktop's light and dark mode is in Personalization.; and Better Manager
使用的語言。桌面的語言在「時間與語言」。 / The language Better Manager uses.
The desktop's language is in Time & language.

## 14. Left to the desktops' settings, or not offered

| Area | Where it stays | Why |
| --- | --- | --- |
| COSMIC's panel, dock, workspaces, window management, frosted glass, X11 compatibility | nowhere in the session | those are COSMIC programs the session does not run (F3); the session's own surfaces carry their own settings |
| Online accounts | GNOME Settings, by link card; not in the session | M8 |
| Night light in the session | nowhere until the compositor offers it | F4, M5 |
| Drawing tablets, Wacom | COSMIC Settings / GNOME Settings by link card | not in the inventory the owner named; a ticket when someone needs it |
| Sharing, remote desktop, search, mobile network (WWAN) | GNOME Settings by link card under GNOME; nothing in the session | not in the inventory; the session has no sharing services of its own |

## 15. Order and sizes

Sizes: **small** is one section or sub-page of a few cards over a store an
existing crate already writes, one backend shared or two trivial ones;
**medium** is a model crate with a service seam and fakes, a sub-page with
groups and dialogs, and two backends; **large** is a new service client with
event streams, several sub-pages and dialogs, both desktops, and a VM check
with real devices.

| Wave | Ticket | Page | Size | Blocked by |
| --- | --- | --- | --- | --- |
| 0 | 231 | Navigation, category landings, link cards, deep links, search | medium | M1, M3 |
| 1 | 232 | Network and internet | large | 231, M4 |
| 1 | 233 | Bluetooth and devices | medium | 231 |
| 1 | 234 | Sound | large | 231 |
| 1 | 235 | Displays (and the session's brightness) | large | 231, 200, 206, M4 |
| 1 | 236 | Power and battery | small | 231, 206, M4 |
| 1 | 237 | Date and time | medium | 231, 206, M4, M6 |
| 1 | 238 | Notifications | medium | 231, 215, 216, M4 |
| 2 | 239 | Keyboard | large | 231, 206, 221 |
| 2 | 240 | Mouse | small | 231, 206 |
| 2 | 241 | Language and region | medium | 231, 203, M6 |
| 2 | 242 | Personalization | medium | 231, 206 |
| 2 | 243 | Accounts | large | 231, M6 |
| 2 | 244 | Accessibility | large | 231, 206 |
| 2 | 245 | Privacy and security | medium | 231, 206 |
| 2 | 246 | Printers | large | 231, M6 |
| 2 | 247 | Lid and power button | medium | 236, M9, ADR 0042 accepted |
| 2 | 248 | Removable media | small | 231 |
| 2 | 249 | Default and startup apps | medium | 231 |
| 2 | 250 | About: device name | small | 231, M6 |
| later | 251 | Night light in the session | small (after M5 (b)) | M5, an upstream compositor with gamma control |
| after wave 1 | 252 | COSMIC Settings out of sight in the session | small | 232–238, M7 |

Wave 1 is what M2 recommends for the "ready to become the default" bar. Every
ticket also has the owner's M1 as a blocker.

## 16. Gaps in the Fluent kit and the icon set

| Gap | Needed by | Ticket |
| --- | --- | --- |
| The navigation pane's items scroll above a fixed footer | the twelve-item pane | 231 |
| Object icons: `bluetooth`, `accessibility`, `speaker`, `microphone`, `display`, `wifi` (with signal levels 0–4), `ethernet`, `vpn`, `hotspot`, `airplane`, `headphones`, `mouse`, `printer`, `clock`, `night-light`, `notifications`, `privacy-camera` | the new pages | 231 draws them to DESIGN.md's rules, in `better-ui` |
| A drag canvas for display arrangement (rectangles with snapping, keyboard-movable) | Displays | 235 |
| A level meter (a thin bar driven by a value stream) | Sound | 234 |
| A QR code element | Wi-Fi share | 232 |
| A date picker beside ticket 87's time picker | Date and time | 237 |
| A password box with a reveal button | Wi-Fi, hidden network, Bluetooth PIN | 232 |

## 17. Faults found while reading

| Fault | Where | Ticket |
| --- | --- | --- |
| Better Manager's brightness card is never drawn in the session: `gsd-power` does not run there | F6, `crates/hardware-platform/src/bus.rs` 37–40 | 235 |
| COSMIC Settings' Super key action can bind Super to programs the session does not install | F3, `window_management.rs` 111–121 | out of scope here; ticket 210's notes are the place (its `system_actions` defaults) |
| `com.system76.CosmicSettings.Notifications.desktop` runs a subcommand 1.11 does not define | `main.rs` 64–162 | upstream's; none of ours |
| The owner's GNOME has no `~/.config/monitors.xml`, so ticket 208's scale carry-over has nothing to read on that machine and must take Mutter's current scale (`GetCurrentState`, 2.0) instead | `ls ~/.config/monitors.xml`, 2026-11-21 | out of scope here; recorded in ticket 208's Notes |
| The bar has no Bluetooth item (top bar spec section 6), so in the session pairing is reached only through a settings page | `design/top-bar-spec.md` 6.2–6.7 | 233 makes Better Manager's page that place; whether the bar gains one is the owner's, listed in ticket 233 |
