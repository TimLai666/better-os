# Spec: the Better OS session

A desktop session of Better OS's own on COSMIC's compositor (`cosmic-comp`),
beside GNOME. The owner chose it on 2026-11-21 (option D of
[ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md)) and answered
that record's decisions C2 to C9 the same day, each as recommended. This file
says what the session is, every process it runs and who provides it, how
Better Manager installs it, switches to it, and removes it in one action each,
how every existing component behaves in it, how gestures follow the fingers,
and when the session is ready to become the default. The work is cut into
tickets 200 to 229, together with the existing tickets 149, 152, 156 to 158,
160, 94, 100, 142, 144, 161, and 174.

Every fact below comes from ADR 0038 and the sources it names, read on
2026-11-21. A statement marked **unverified** is one ADR 0038 lists as not run;
the ticket named beside it settles it, and its first step is the check.

## Goal and problem

Everything the owner has asked of the desktop since November (a top bar, a
taskbar, a notification island and center, one corner radius for every
window, a desktop with stacks, and gestures whose result follows the fingers,
can be turned back halfway, and settles when they lift) is reached on GNOME 46
only through shell JavaScript and a further widening of the GJS exception, and
the gesture requirement means driving GNOME Shell's private transition methods
once per frame from outside. On `cosmic-comp` each of these is either already
done by the compositor or is a Better OS Rust process on layer-shell.

When this is done, a person on Zorin OS 18 or Ubuntu 24.04 with Better OS
installs the Better OS session in one action in Better Manager, logs in to it
from GDM, works a whole day in it with Better OS's own bar, taskbar, launcher,
notifications, desktop, and finger-following gestures, and removes it in one
action, with GNOME exactly as it was. GNOME stays installed and the default
until the session meets the bar in "Ready to become the default".

## Scope

### S1 — A person can install the Better OS session and switch to it in one action
- Done when: on a Zorin OS 18 and an Ubuntu 24.04 machine with Better OS
  installed, one action in Better Manager (Apps › Better OS session › Install)
  installs everything the session needs and makes it the person's session for
  the next login, with no terminal, no further setting, and no second step.
- Done when: the page says before and after the action, in plain words, that
  the switch takes effect at the next login, and offers 立即登出 / Log out now
  in the same confirmation.
- Done when: GDM lists 「Better OS」 and starts it; the person lands in the
  session with their wallpaper, dark or light mode, display scale, keyboard
  layouts, input sources, default applications, and `~/Desktop` as they had
  them in GNOME.
- Done when: on Ubuntu 22.04 the component is not offered, and the page says
  why.

### S2 — A person can switch between the Better OS session and GNOME in one action each way
- Done when: 改用 GNOME / Use GNOME and 改用 Better OS / Use Better OS on the
  component page each set the session for the next login, with Log out now
  beside them, and GDM's own session menu does the same.
- Done when: switching never writes GNOME's settings (rule in "Carrying
  settings over").

### S3 — A person can remove the session in one action and get the previous desktop back exactly
- Done when: removal sets the person's session back to the one captured at
  install, purges every package the install added, the System76 source, its
  key, and its pin, and deletes the session's per-person settings unless "Also
  delete my settings and history" is unticked.
- Done when: after removal and a login to GNOME, a file-by-file comparison of
  the person's home and of `/etc`, `/usr`, and `/var/lib/dpkg/status` against
  a snapshot taken before install shows no difference outside the paths the
  spec lists as GNOME's own (logs, caches GNOME itself writes).
- Done when: removal started from inside the Better OS session finishes after
  the person logs out (see "Removing from inside the session").

### S4 — A person can work a whole day in the session
- Done when: every duty GNOME performs without being asked has a provider in
  the session (the process table below), and acceptance flow 2 passes.
- Done when: the top bar, taskbar, launcher, window switcher, overview,
  notification island and center, and desktop are Better OS's own layer-shell
  programs and none of them is COSMIC's.
- Done when: Traditional Chinese is typed with Zhuyin in a Better OS window,
  Firefox, and an Electron application, a menu opens while the input method
  runs, and locking and unlocking with a composition open works (ADR 0038's
  four upstream input faults: issues 2947, 2952, 2702, 2778).

### S5 — Every Better OS component works in the session or says why it does not
- Done when: each row of "Every component in the session" behaves as the row
  says, with its tests.

### S6 — Every system gesture follows the fingers, can be turned back halfway, and settles when they lift
- Done when: the Mac-style preset's system gestures (four fingers up for the
  overview, four fingers down for the current application's windows, four
  fingers left and right for workspaces, thumb and three fingers pinching in
  for Better Launcher and spreading out for the desktop) and the notification
  center's edge swipe each move their surface with the fingers, reverse when
  the fingers go back, cancel below the threshold, and settle with the
  release velocity.
- Done when: the added latency from the compositor's input event to the
  surface's frame is measured and within the budget in "Gestures"
  (for the implementer: the owner may change the budget).
- Done when: before the gesture patch is installed, the Touchpad page says
  exactly which gestures the released compositor gives (four fingers left and
  right switch workspaces, following the fingers) and that the others need the
  Better OS compositor.

### S7 — The session follows COSMIC's releases without breaking the person's machine
- Done when: an Epoch release that needs a package outside the pinned closure
  is held back with a visible reason in Better Manager's Updates, never
  installed half, and never pulls a base package from System76's source.
- Done when: the gesture patch is rebased, built, and released for each Epoch
  release the session takes (ADR 0039).

### S8 — The owner can make the session the default once it has earned it
- Done when: the bar in "Ready to become the default" is met and recorded, and
  the owner has accepted ADR 0040.

## Not doing

- **Ubuntu 22.04.** System76's `jammy` compositor is the January 2025 alpha and
  its `noble` builds need `libc6 (>= 2.39)` (ADR 0038). Reconsidered when
  22.04 leaves the support matrix or someone builds the closure for it.
- **Stock COSMIC as a second desktop** (options B and C). COSMIC's panel,
  dock, launcher, app library, notifications, workspaces view, and COSMIC
  Files' desktop are not installed: they are not in the 14-package closure.
- **`cosmic-greeter` as the login screen** (C3: GDM stays). Its package is
  installed because the locker is in it and `cosmic-settings` depends on it;
  its display-manager question is preseeded to `gdm3`.
- **A compositor written from nothing** (ticket 143).
- **Better OS's own APT repository or signing implementation.** C2 allows
  exactly System76's source with its one key and a pin limited to the closure;
  nothing else.
- **Per-application window attention in the session.** `cosmic-comp` has one
  machine-wide `activation_policy` and no urgent state (ADR 0038); the page
  says so. Reconsidered when upstream offers a per-application hook.
- **Moving COSMIC Settings' pages into Better Manager now** (C9). Ticket 229
  designed it: `design/system-settings-spec.md`, tickets 231 to 252, and the
  owner's decisions M1 to M9 at its top. Its M2 proposes an eighth item for
  "Ready to become the default" (the wave 1 pages); the list below changes
  only if the owner accepts it.
- **Title bars drawn in Better OS's style by a compositor built with
  `cosmic_comp::run(Hooks)`.** The hooks are "an _unstable_ interface" and draw
  in libcosmic, not GPUI. C5 takes the compositor's own title bar.
- **IBus in the session.** IBus 1.5.29 offers `cosmic-comp` no Wayland input
  method (ADR 0038); C4 takes Fcitx 5 with Chewing.
- **New GNOME-only surfaces** while the session is being built: tickets 151,
  159, and 161's GNOME build wait (ADR 0038, "Which of tickets 150 to 161
  should wait"). Ticket 131's fix stays, because GNOME is the default until
  S8.

## The session

### What it is

`cosmic-comp` as the compositor, the COSMIC daemons Better OS does not
replace, and Better OS's own surfaces and services, started by a Better OS
session launcher under a systemd user target. `cosmic-session` is not used:
its list of components is fixed in code (ADR 0038, `src/main.rs` lines
270–356).

```text
GDM ──> /usr/share/wayland-sessions/better-os.desktop
         Exec=/usr/libexec/better-os/session/start          (Rust, better-os-session)
           sets the environment below, then execs
         cosmic-comp /usr/libexec/better-os/session/start --ready
           (cosmic-comp runs that program once it is up: src/lib.rs 125–147)
             --ready: imports WAYLAND_DISPLAY, DISPLAY, XDG_CURRENT_DESKTOP,
             XDG_SESSION_DESKTOP, DCONF_PROFILE into the user manager and
             D-Bus activation, starts better-os-session.target, waits;
             on exit stops the target and unsets what it imported

better-os-session.target   BindsTo=graphical-session.target
                           Wants=graphical-session-pre.target
                           Wants=xdg-desktop-autostart.target
  ├── COSMIC (taken as released, from System76's pinned source)
  │     cosmic-settings-daemon, cosmic-osd (polkit agent, OSD), cosmic-idle,
  │     cosmic-bg, the cosmic-greeter locker, xdg-desktop-portal-cosmic
  ├── Ubuntu's archive
  │     fcitx5 with fcitx5-chewing, tracker-miner-fs-3 where installed,
  │     gnome-keyring (unlocked by GDM's PAM), xdg-desktop-portal-gtk, Orca
  └── Better OS
        session surfaces: better-top-bar (+ top-bar-xembed), better-taskbar,
        better-launcher (resident), window switcher, overview, notification
        service, Better Files desktop
        every existing Better OS user unit, unchanged, through
        graphical-session.target
```

### The environment the launcher sets

| Variable | Value | Why |
| --- | --- | --- |
| `XDG_CURRENT_DESKTOP` | `BetterOS:COSMIC` | COSMIC's portal is `UseIn=COSMIC` and COSMIC's daemons look for it; `BetterOS` first so portal configuration and Better OS's own session matching pick the session's own rules (ADR 0038, "Portals") |
| `XDG_SESSION_DESKTOP`, `DESKTOP_SESSION` | `better-os` | names the session; `better_core::desktop` reads `DESKTOP_SESSION` only as a fallback |
| `XDG_SESSION_TYPE` | `wayland` | set by GDM; checked, not set |
| `DCONF_PROFILE` | `betteros` | `/usr/share/dconf/profile/betteros` is `user-db:betteros` then `user-db:user`: every GSettings write in the session, `cosmic-settings-daemon`'s GTK theme and font syncing included, goes to `~/.config/dconf/betteros`, and GNOME's database is only read. This is COSMIC's own arrangement (`DCONF_PROFILE=cosmic`, ADR 0038) with Better OS's name |
| `XDG_DATA_DIRS` | `/usr/share/better-os/session` prepended | the session's defaults for COSMIC's configuration are package data there, if `cosmic-config` reads system defaults through `XDG_DATA_DIRS` (**unverified**, ticket 201; otherwise ticket 203 writes them per person through the cosmic-config adapter with capture and restore) |
| `GTK_IM_MODULE`, `QT_IM_MODULE`, `XMODIFIERS` | `fcitx`, `fcitx`, `@im=fcitx` | Fcitx 5 for GTK, Qt, and X11 clients (C4). Not set for Wayland-native text-input-v3 clients, which do not need it |

Nothing the launcher imports outlives the session in the user manager: it is
unset on exit, as sway's systemd integration does, so a GNOME login after a
Better OS one starts with GNOME's own environment (**unverified**, ticket 203
checks it by logging out of one and into the other in the VM).

### Every process the session runs, and who provides it

| Duty | Provided by | Kind | Evidence |
| --- | --- | --- | --- |
| Compositor, XWayland (rootless), window clipping to one radius, the four-finger workspace switch | `cosmic-comp` | COSMIC as released (C8), later Better OS's patched build (ticket 223) | ADR 0038: seen in Pop's image; on Zorin's libraries **unverified**, ticket 201 |
| Login screen | GDM | Zorin's / Ubuntu's, unchanged (C3) | GDM starting a non-GNOME session: **unverified**, ticket 201 |
| Session start and stop, environment, systemd target | `better-os-session` (`/usr/libexec/better-os/session/start`) | Better OS, new (ticket 203) | — |
| Settings daemon (theme, fonts, GTK and Qt sync, brightness keys) | `cosmic-settings-daemon` | COSMIC | seen running in Pop |
| Portals: FileChooser, Screenshot, ScreenCast, RemoteDesktop, Settings, Access | `xdg-desktop-portal-cosmic` | COSMIC | file chooser seen opening for a GPUI window |
| Portals COSMIC lacks | `xdg-desktop-portal-gtk` | Ubuntu | Pop's `cosmic-portals.conf`: `default=cosmic;gtk;`; Better OS ships `betteros-portals.conf` with the same order (ticket 203), and ticket 100 later puts Better Files first for FileChooser |
| Secret portal and keyring | `gnome-keyring` (unlocked at login by GDM's PAM stack), `oo7-portal` where installed | Ubuntu | PAM unlocking in a non-GNOME session: **unverified**, ticket 201 |
| Polkit agent, volume and brightness pop-ups | `cosmic-osd` | COSMIC | `src/subscriptions/polkit_agent.rs` |
| Screen lock | the locker in `cosmic-greeter` over `ext_session_lock_manager_v1`; `loginctl lock-session` locks | COSMIC | seen in Pop; which process answers logind's `Lock` outside `cosmic-session`: **unverified**, ticket 201 |
| Blanking, idle, suspend on idle; `org.freedesktop.ScreenSaver` | `cosmic-idle` | COSMIC | honours only `org.freedesktop.ScreenSaver.Inhibit` (ADR 0038) |
| Wallpaper, also on the lock screen | `cosmic-bg` | COSMIC | — |
| Display configuration | `cosmic-randr`, COSMIC Settings' Displays page (C9) | COSMIC | — |
| System settings: display, network, Bluetooth, sound, users, region | COSMIC Settings (C9) | COSMIC | Better OS's links to "… Settings" open its pages (ticket 160) until each Better Manager page of `design/system-settings-spec.md` lands; COSMIC Settings has no night light, printers, notifications, privacy, or hotspot, and GNOME Settings refuses to run outside GNOME (that file's F1, F2) |
| Input method | Fcitx 5 with `fcitx5-chewing` over `zwp_input_method_v2` and text-input-v3; XIM for X11 clients | Ubuntu (C4) | Zhuyin seen typed into Better Launcher in Pop; the four upstream faults: **unverified**, ticket 201 |
| Network secrets, Bluetooth pairing | Better OS's top bar: a NetworkManager secret agent and a BlueZ agent | Better OS, new (ticket 217) | COSMIC keeps these in its panel applets and Settings (ADR 0038, read, not run) |
| Notifications (`org.freedesktop.Notifications`) | Better OS's notification service, island and center | Better OS, new (tickets 215, 216) | `cosmic-notifications` is not installed |
| Tray watcher (`org.kde.StatusNotifierWatcher`) and old X11 icons | the top bar's watcher (ticket 156) and `top-bar-xembed` (ticket 149) | Better OS | — |
| Top bar | `better-top-bar` on layer-shell | Better OS (tickets 152, 156–158, 160) | — |
| Taskbar | `better-taskbar` on layer-shell | Better OS, new (ticket 213) | — |
| Launcher (Super, Super+Space) and the application grid (Super+A) | Better Launcher, resident, on layer-shell | Better OS (tickets 94, 210) | — |
| Window switcher (Alt+Tab) | Better OS's switcher on layer-shell | Better OS, new (ticket 211) | COSMIC's switcher is part of `cosmic-launcher`, not installed (to confirm in ticket 201's inventory) |
| Overview and workspaces | Better OS's overview on layer-shell, windows through `ext-image-copy-capture`, workspaces through `ext_workspace_v1` | Better OS, new (ticket 212) | `cosmic-workspaces` is not installed |
| Desktop icons and stacks, `org.freedesktop.FileManager1` | Better Files on the background layer (C6) | Better OS (tickets 214, 144, 129) | COSMIC Files' desktop process is not started |
| Gestures | `better-touchpad-gestured` with a `cosmic-comp` backend (tickets 223, 224); the edge swipe through the scroll pass-through (ticket 225) | Better OS | — |
| File search index | `tracker-miner-fs-3`, started by the session target when installed | Ubuntu | Zorin runs it; Pop has none. How it is started outside GNOME: **unverified**, ticket 201 |
| Accessibility | Orca over AT-SPI, `cosmic_a11y_manager_v1` | Ubuntu, COSMIC | depth not assessed (ADR 0038); a Better OS window read by Orca in the session: ticket 222 |
| Screen sharing | `xdg-desktop-portal-cosmic` ScreenCast over `ext-image-copy-capture` and PipeWire | COSMIC, Ubuntu | on noble's PipeWire 1.0.5: **unverified**, ticket 201 |
| XDG autostart | `systemd-xdg-autostart-generator`'s `xdg-desktop-autostart.target`, entries filtered by `OnlyShowIn`/`NotShowIn` against `XDG_CURRENT_DESKTOP` | systemd | which of Zorin's entries start: **unverified**, ticket 201 |
| Power: lid, suspend, battery | logind, UPower, power-profiles-daemon | Ubuntu, unchanged | — |
| Media keys (volume, brightness, screenshot) | `cosmic-comp`'s shortcuts and `system_actions` | COSMIC | that every action the session binds names an installed program: ticket 201's inventory, ticket 203's test |

Ticket 201 also records every duty it finds that this table misses; each gap
it finds gets its own ticket in the same change.

### How Better OS's existing user units bind to the session

Every Better OS user unit is `PartOf=`, `After=`, and `WantedBy=`
`graphical-session.target` (`packaging/*/*.service`, checked by
`verify-deb.sh`). `better-os-session.target` has `BindsTo=graphical-session.target`,
so the same units start and stop with the Better OS session as with GNOME's,
with no change to any unit and no `Condition` per desktop. A part that is
desktop-specific chooses its backend at run time from `better_core::desktop`
(the one place the session is read), so one installed unit serves both
sessions:

- `better-touchpad-gestures.service` waits for the GNOME adapter under GNOME
  and uses the compositor's gesture protocol in the session; with the released
  compositor it stays resident, idle, and says so (ticket 219).
- `better-window-attention.service` keeps the charger switch in both sessions
  and does the GNOME relay's work only under GNOME (ticket 220).
- `better-awake.service` adds the ScreenSaver inhibitor in the session
  (ticket 218).

The session's own surfaces are units of their own packages, enabled by
`manager-core` like any component unit (AGENTS.md), `PartOf=better-os-session.target`
and `WantedBy=better-os-session.target`, so they never start under GNOME.
GNOME's own units (`org.gnome.Shell@wayland.service`, `gnome-session-*.target`,
`org.gnome.SettingsDaemon.*`) are wanted by GNOME's targets, not by
`graphical-session.target`, so they do not start in the session; which other
packages' units on Zorin are `WantedBy=graphical-session.target` and start in
the session is **unverified**, ticket 201.

## Installing, switching, and removing

### Packages and where they come from (C2)

| Packages | Source | How |
| --- | --- | --- |
| The 14-package closure: `adw-gtk3`, `cosmic-bg`, `cosmic-comp`, `cosmic-greeter`, `cosmic-greeter-daemon`, `cosmic-icons`, `cosmic-idle`, `cosmic-osd`, `cosmic-randr`, `cosmic-settings`, `cosmic-settings-daemon`, `cosmic-theme-bundles`, `pop-sound-theme`, `xdg-desktop-portal-cosmic` | System76's `https://apt.pop-os.org/release` `noble`, signed by `63C4 6DF0 140D 7389 6142 9F4E 204D D8AE C33A 7AFF` | one deb822 source file, one keyring holding only that key, and a pin: every package from that origin at priority −1 except the 14 names at 500. Every other dependency resolves to the host's own `noble` versions (ADR 0038, `session-closure-on-host-noble.txt`). Declared metadata only: the binaries running on Zorin's Wayland 1.22, Mesa 25.2, and PipeWire 1.0.5 is **unverified**, ticket 201 |
| `fcitx5`, `fcitx5-chewing`, `fcitx5-frontend-gtk3`, `fcitx5-frontend-gtk4`, `fcitx5-frontend-qt5`, `fcitx5-config-qt` | Ubuntu's archive (`universe`) | ordinary dependencies of the session package |
| `better-os-session` (launcher, session file, targets, dconf profile, portal configuration, the session's COSMIC defaults) and the surfaces' packages | Better OS's release assets, verified by checksum like every component | as every component |
| `cosmic-comp` with the gesture patch, once ticket 223 ships | Better OS's release assets (ADR 0039) | `cosmic-comp` leaves the pin's list in the same release, so System76's build can never replace it |

Whether System76 publishes `arm64` for these 14 in `noble` is **unverified**;
ticket 201 reads it, and the manifest lists `arm64` only if it does.

The privileged steps (source, key, pin, display-manager preseed, installing
and purging the closure) are `manager-daemon`'s, with the source, the key's
fingerprint, the key bytes, and the 14 names compiled into the daemon and
nothing taken from a manifest (ticket 204; architecture.md, "The Better OS
session"). The display-manager question `shared/default-x-display-manager`
is preseeded to `gdm3` before `cosmic-greeter` is installed, so
`display-manager.service` is never relinked.

### The component

`components/manifests/better-os-session.yaml`: id `better-os-session`,
display name 「Better OS 工作階段」 / "Better OS session", `component_type:
enhancement` with nothing in `replaces` or `enhances` and `icon: generic` (the
schema's closed sets, as Better Git's manifest explains), `targets.releases:
[24.04]`, `distributions: [ubuntu, zorin]`, `restart: logout`, health checks
for the source and pin being exactly the compiled-in ones, the closure
installed, the session file present, and the person's AccountsService session
(ticket 205). `paths` names the person's session data: `~/.config/cosmic/`,
`~/.local/state/cosmic/`, `~/.local/state/cosmic-comp/`,
`~/.config/dconf/betteros`, `$XDG_STATE_HOME/better-os/session/`, and
`~/.config/fcitx5/` only when the install created it (recorded at install).
Default integrations for the session name `sessions: [betteros]`, never
`[cosmic]`, so a declaration meant for the session never applies elsewhere.

### Switching means the next login

A running GNOME session cannot become another session. "Switch" means: the
person's AccountsService session (`SetSession`) is set, which GDM reads to
preselect the session at the next login. The component page says so in plain
words:

| State | zh-TW | English |
| --- | --- | --- |
| Installed, GNOME is the session | 下次登入時會使用 Better OS 工作階段。 / 立即登出 | You'll be in the Better OS session the next time you log in. / Log out now |
| In the Better OS session | 你正在使用 Better OS 工作階段。 / 改用 GNOME | You're using the Better OS session. / Use GNOME |
| GNOME chosen for next login | 下次登入時會回到 GNOME。 / 立即登出 | You'll be back in GNOME the next time you log in. / Log out now |
| 22.04 | 這個版本的 Ubuntu 無法使用 Better OS 工作階段，需要 Ubuntu 24.04 或以它為基礎的系統。 | The Better OS session needs Ubuntu 24.04 or a system based on it. |

AccountsService's `SetSession` on the person's own account is the person's
own data, authorized by AccountsService's own polkit action, so it is called
from `manager-core` without `manager-daemon`, as the top bar calls
NetworkManager (ticket 205). Log out now asks the current session's own
logout (GNOME's session manager under GNOME, the session launcher's in the
Better OS session).

### Carrying settings over

Read from GNOME's dconf database, from `monitors.xml`, and from IBus's
settings, and written to the session's own places, at install and at every
Use Better OS. Nothing is ever written back to GNOME, so GNOME is exactly as
the person left it (owner decision S1 below):

| What | From (GNOME) | To (session) |
| --- | --- | --- |
| Wallpaper | `org.gnome.desktop.background picture-uri`, `picture-uri-dark` | `cosmic-bg`'s configuration |
| Dark or light | `org.gnome.desktop.interface color-scheme` | `com.system76.CosmicTheme.Mode` `is_dark` |
| Display scale | `~/.config/monitors.xml` | `cosmic-comp`'s output configuration (its path and format: ticket 201) |
| Keyboard layouts | `org.gnome.desktop.input-sources sources`, its `xkb` entries | `com.system76.CosmicComp` `xkb_config` |
| Input sources | `sources`' `ibus` entries (`chewing` on the owner's machine) | Fcitx 5's profile, Chewing for `ibus:chewing`, each engine with no Fcitx 5 counterpart listed on the page rather than dropped silently |
| Input-source switch key | ticket 94's `<Alt>Shift_L` | Fcitx 5's trigger and enumerate keys (**unverified** where Fcitx 5 reads them, ticket 208) |
| Default applications | `mimeapps.list` | shared, untouched |
| `~/Desktop` | the folder | shared, untouched; never moved |

### Removing, and returning to the previous desktop

1. Units of the session's surfaces stopped and disabled (`UserUnitController`).
2. The person's AccountsService session set back to the one captured at
   install (or GNOME's default when that session no longer exists).
3. `manager-daemon` purges the closure, the session package, the source, the
   key, and the pin; GDM is untouched; Fcitx 5 packages the install added are
   purged unless something else depends on them.
4. The person's session data in `paths` deleted unless "Also delete my
   settings and history" is unticked.
5. Health check: no package from System76's origin remains, `apt-cache policy`
   shows no source from it, and the person's session reads the captured one.

GNOME's dconf database is never written by the session's daemons (their
profile writes to `~/.config/dconf/betteros`), so GNOME comes back as it was.
Whether COSMIC's daemons or COSMIC Settings write any file GNOME reads (for
example `~/.config/gtk-4.0/` or `~/.config/gtk-3.0/settings.ini`) is
**unverified**; acceptance flow 3's home comparison settles it (ticket 205),
and the session's defaults turn such syncing off where it can be turned off.

#### Removing from inside the session

The compositor cannot be purged under the person's open session. Remove
pressed inside the session sets the next session to the captured one, says
登出後就會完成移除。 / Removal finishes when you log out., and offers Log out
now; `manager-daemon` purges in a transient unit once logind reports no open
session of the Better OS session for any user, as `daemon-handoff` waits for
an idle daemon (ADR 0028's shape). Until then the component reads 正在等待登出
/ Waiting for log out (owner decision S2 below).

#### Several people on one machine

The session is a machine-wide install. Removal is refused while another person
has a Better OS session open, with their name; every other person whose
AccountsService session is the Better OS session is set back to GNOME's
default by the daemon at purge, and Better Manager tells them at their next
start.

## Every component in the session

| Component and its crates | Works unchanged | Needs a session backend | GNOME-only, stays behind |
| --- | --- | --- | --- |
| Better Manager (`manager-gui`, `manager-cli`, `manager-core`, `manager-platform`, `manager-daemon`) | windows, the daemon, catalog, plans, user units, the login screen watch while GDM stays (ADR 0033) | `manager_platform::shell_extensions` does nothing in the session (no shell); the control center's Touchpad, Window attention, and Default apps pages show the session's backends | `shell_extensions` |
| Better OS windows (`better-ui`) | every GPUI window draws, scales, takes input on `cosmic-comp` (five seen) | ask for server-side decorations and skip `system_frame` in the session (C5, ticket 209) | `better_ui::system_frame`'s GTK frame |
| Better Files (`files-*`) | the window, jobs, previews, drives (`drive-mounts`, UDisks2, GVfs seen) | the desktop on the background layer (C6, ticket 214), `org.freedesktop.FileManager1` (no COSMIC Files applet in the session, ticket 129's mechanism), the FileChooser portal through `betteros-portals.conf` (ticket 100) | — |
| Better Launcher (`launcher-*`) | the overlay's content, ranking, single instance on `org.betteros.Launcher1` | a layer-shell panel; Super, Super+Space, Super+A bound in the session's shortcuts (ticket 210) | `launcher-platform/src/shortcut.rs`'s GNOME keybinding |
| Better Monitor (`monitor-*`) | service, window, tray (seen in COSMIC's watcher) | the `@<random>` unit grouping (ticket 174) | — |
| Better Awake (`awake-*`) | service, tray, logind inhibitors (suspend, lid) | `org.freedesktop.ScreenSaver.Inhibit` for blanking and a fullscreen trigger from `zcosmic_toplevel_info_v1` (ticket 218) | — |
| Better Touchpad (`touchpad-core`, `touchpad-platform`, `touchpad-gui`, `touchpad-views`) | per-device profiles' model | a `CosmicBackend` writing `com.system76.CosmicComp` `input_touchpad` and `input_devices` (ticket 219) | `touchpad-platform/src/gnome.rs` (nine keys) |
| Scroll pass-through (`touchpad-scroll`) | the service, unchanged | `scroll_config.scroll_factor` written instead while both axes have one factor; the evdev grab only when they differ (ticket 219); the edge swipe (ticket 225) | — |
| Gestures (`touchpad-gestures`, `touchpad-gesture-service`, `touchpad-session`, `better-actions`) | the recognizer, presets, store, the one activation and cancellation rule | a `cosmic-comp` backend on the gesture protocol and transitions to the surfaces (tickets 223, 224); "what this compositor gives" before the patch (ticket 219) | `adapters/gnome-shell-touchpad/`, `touchpad-session`'s adapter bus, `SuppressBuiltInGestures` |
| Window attention (`window-attention-*`) | the charger switch | the default choice as `cosmic-comp`'s `activation_policy`; per-application choices shown unavailable (ticket 220) | the `WindowAttention` relay, per-application choices |
| Better Defaults (`defaults-core`, `defaults-platform`, `defaults-store`) | the XDG and `mimeapps.list` adapters | a cosmic-config adapter (ticket 206); the dconf writer writes the database the session reads (ticket 207) | the GNOME keys of the shipped manifests (`sessions: [gnome]` already keeps them out) |
| Hardware (`hardware-*`) | sysfs, PPD, UPower, the daemon's two writes | — | `gnome-settings-daemon` endpoints (UPower's keyboard backlight endpoint is already the fallback) |
| Storage (`storage-*`) | unchanged | — | — |
| Better Git (`git-*`), App chooser (`app-chooser-*`), App catalog (`app-catalog-*`) | unchanged (the catalog reads `XDG_DATA_DIRS`, which the launcher extends) | — | — |
| The top bar (`top-bar-*`, tickets 149–160) | core, service, view, tray, status area, clock | a layer surface (152), the session's sources (160), agents (217), notifications from Better OS's service (158, 215) | the panel relay (151, waiting) |
| The shell adapter (`adapters/gnome-shell-adapter/`) | — | — | every relay in it |
| Better Input (tickets 180–191) | engine core, models, voice service, voice key | reaching applications through the session's input method framework (ticket 221; owner decision S4) | the IBus engine (183), IBus restart (185), GNOME's `sources` adapter (184) |

**The dconf finding.** `defaults-platform` always writes
`/ca/desrt/dconf/Writer/user` and reads `~/.config/dconf/user`
(`dconf_writer.rs`, `dconf.rs`). In a session whose `DCONF_PROFILE` puts
another writable database first, a key it writes lands in GNOME's database and
is hidden by any value the session's database holds (ADR 0038). Ticket 207
made the adapter write and read the first `user-db` of the profile the
target session uses: `defaults_platform::dconf_profile` follows dconf's own
lookup (a mandatory profile, then `DCONF_PROFILE`, then the runtime and `user`
profiles, then `user-db:user`; names through `/etc/dconf/profile/` and then
`XDG_DATA_DIRS`), and the GNOME adapters and Better Touchpad's backend read
that database and write through its writer object (`/ca/desrt/dconf/Writer/<name>`,
or `/ca/desrt/dconf/<name>` for a `service-db`, and the safe writer dconf 51
uses for a home on NFS). A value shown from a `user-db` beneath the session's
own is displayed (launcher lists, tool entries, Better Touchpad's settings and
its shortcut check) but captured as not held, so a restore resets the
session's key instead of copying the value into it. Every defaults record
names the database it was captured from and written to (a 0.2.16 record names
none and means GNOME's), and status, verify, and restore use that database in
whichever session they run. A first `system-db` or `file-db`, a named profile
that cannot be read, and a process whose `XDG_CURRENT_DESKTOP` names COSMIC or
Better OS with no `DCONF_PROFILE` (both sessions set it, so such a process does
not have the session's environment) are reported with a sentence that says
what to do, and nothing is written; an unreadable `user` profile falls back to
`user-db:user` as dconf does.

## Gestures

### What the released compositor gives (until ticket 223)

`cosmic-comp` takes every swipe of three or more fingers for itself while its
own workspace overview is closed: three fingers do nothing, four fingers
switch workspaces in the layout's direction, following the fingers through
`update_workspace_delta` and settling with their velocity in
`end_workspace_swipe`; pinches and holds go to the focused window (ADR 0038,
`src/input/mod.rs` lines 1195–1350). So in the first session the four-finger
workspace switch already meets the owner's requirement, and nothing else
does: no overview, launcher, desktop, or application-windows gesture reaches
Better OS, and the thumb-and-three pinch goes to the focused window. The
Touchpad page says exactly this. **The owner's requirement that every gesture
has the macOS transition is met only when the gesture patch lands (ticket 223
and 224); the session does not become the default before then.**

### The one compositor change (C8, ADR 0039)

A private Wayland protocol, `betteros-gesture-unstable-v1`, whose XML lives in
this repository (GPL-3.0-or-later) and is implemented by the patch in
`cosmic-comp` (GPL-3.0-only, ADR 0039): a new module under
`src/wayland/protocols/` and its handler, the three swipe arms and the pinch
arms of `src/input/mod.rs`, and one line in `state.rs`. ADR 0038 estimates
400 to 800 lines.

### The contract between compositor and gesture service

```text
global zbetteros_gesture_manager_v1 (version 1)
  offered only to a client outside a security context
  request claim_swipe(new_id zbetteros_gesture_claim_v1, fingers: uint)
  request claim_pinch(new_id zbetteros_gesture_claim_v1, fingers: uint)
  request get_workspace_driver(new_id zbetteros_workspace_driver_v1)
  request destroy
  errors: not_allowed (the client is not the gesture service),
          already_claimed (another claim holds that kind and count),
          invalid_fingers (outside 3..=5 for swipes, 2..=5 for pinches)

interface zbetteros_gesture_claim_v1
  event begin(serial: uint, time: uint, fingers: uint, device: string)
        device = libinput's sysname of the touchpad ("event7"): ADR 0015
  event update(time: uint, dx: fixed, dy: fixed,
               dx_unaccel: fixed, dy_unaccel: fixed)        swipe
  event pinch_update(time: uint, dx: fixed, dy: fixed,
                     scale: fixed, rotation: fixed)          pinch
  event end(time: uint, cancelled: uint)
  event hold_begin(time: uint, fingers: uint)                fingers rest on the pad
  event hold_end(time: uint, cancelled: uint)
  request release (destructor)

interface zbetteros_workspace_driver_v1
  request begin(serial: uint, orientation: uint)   serial of a claimed gesture's begin
  request update(delta: fixed)                     workspaces, 1.0 = one workspace
  request end(velocity: fixed, cancel: uint)       workspaces per second
  event done(result: uint)                         0 committed, 1 cancelled, 2 refused
  request destroy
```

Rules the patch and its tests hold to:

- **Who may claim.** The compositor accepts a claim only from a client whose
  executable (`SO_PEERCRED`, then `/proc/<pid>/exe`) is
  `/usr/bin/better-touchpad-gestured` and which is not in a security context;
  anything else gets `not_allowed`. This is ADR 0035's relay rule applied to
  a Wayland client.
- **Only while claimed.** While a claim for a kind and finger count exists,
  every such gesture goes to the claim and neither to the compositor's own
  handling nor to a window. When the claim is released or its client
  disconnects, the compositor's own behaviour returns at once, mid-gesture
  included (the gesture in progress ends with `cancelled`).
- **Facts, not decisions.** The compositor reports what libinput reported and
  performs what it is asked; it holds no threshold, no direction mapping, and
  no action. The recognizer stays one place (`touchpad-gestures`), as on
  GNOME.
- **Driving the workspace switch** calls the compositor's existing
  `update_workspace_delta` and `end_workspace_swipe`, so the workspace
  transition is the compositor's own animation, driven by Better OS's
  directions and finger counts.
- **Versioned.** A compositor without the global is the released one; the
  service then runs in "released compositor" mode.

### From the service to the surfaces

The gesture service turns claim events into frames for the one recognizer
(`touchpad-gestures::ingest`), which already sees continuous progress.
Transitions go to the surface that draws them over the service's own
`org.betteros.TouchpadGestures1`:

```text
signal TransitionBegin(id: u, kind: u)       kind: closed enum — overview,
                                             application windows, launcher,
                                             desktop, notification center
signal TransitionUpdate(id: u, progress: d)  0.0 closed .. 1.0 open; may go
                                             back below 0 (turned back)
signal TransitionEnd(id: u, commit: b, velocity: d)
```

Each surface accepts these only from the name's owner running
`/usr/bin/better-touchpad-gestured` (ADR 0035's check). The commit decision
(threshold, velocity, cancellation) is the recognizer's; a surface only draws:
it follows `progress` each frame, and on `TransitionEnd` settles to 0 or 1
with a spring started at `velocity`. Fingers resting on the pad during a
settle (`hold_begin`) catch it where it is. With reduced motion on, the
surface follows the fingers by opacity rather than movement.

**Latency budget.** The added latency from the compositor's input event to the
surface's next committed frame is at most one frame at the display's refresh
rate at the 95th percentile (16.7 ms at 60 Hz), measured by ticket 202 for the
prototype and by ticket 224 for the shipped path (owner decision S5 below).

### The edge swipe

Not from any Wayland protocol or from inside the compositor: libinput reports
a touchpad swipe as the fingers' centre only (ADR 0038). It comes from the
scroll pass-through's evdev view of the pad (ticket 161's option (a)), the
same on GNOME and in the session; ticket 225 builds it once the owner answers
ticket 161's N3.

## Surfaces on layer-shell (ticket 200)

Every session surface opens its windows through `better_ui::layer`:
`LayerSurfaces` keeps one GPUI window per output (or one, for the launcher
and the switcher), opens one on an output that appears, drops the one the
compositor closed when its output went, and changes a surface in place where
GPUI can (exclusive zone and edge, a fixed size) and by opening a new one
before closing the old where it cannot (namespace, layer, anchor, margin,
keyboard mode, and any size change of a surface that fills an axis).
`apply_input_region` keeps input to what is drawn, and `layer::clock` moves a
panel inside a fixed surface (slide, follow the fingers, settle on release;
the settle constants are there and nowhere else). It sets GPUI's quit mode to
explicit: a surface process outlives its windows. `better_core::desktop`'s
`CurrentDesktop::is_better_os_session` is the one place a component asks
whether it runs in the session.

What was seen on `cosmic-comp` 0.1~1795167359 (Pop `noble`, built
2026-11-20), nested, at scale 1, with GPUI at `ae394f3`: all four layers stack
as the protocol says around a window; all sixteen anchor combinations land
where they should; a maximized window leaves the exclusive zone free and is
reconfigured when the zone changes, in place or by a new surface; keyboard
`none` never takes focus, `on-demand` takes it when it maps and when clicked
and gives it up to a clicked window, `exclusive` keeps every key; a popup
parented to a surface takes the keyboard even from a `none` surface and gives
it back when it closes; the input region lets clicks through outside it.
Ticket 200's notes have each one with how it was seen.

**Three GPUI defects at `ae394f3` bear on the surfaces**, all three still in
Zed's `main` on 2026-11-21 except the second:

1. A surface that fills an axis (size 0, the top bar, the taskbar, the
   desktop) is killed by a protocol error when the compositor sends its
   fractional scale before its first configure: GPUI rescales and sends
   `wp_viewport.set_destination(0, h)` (`gpui_linux` `wayland/window.rs`,
   `set_size_and_scale`, lines 1286–1290). `cosmic-comp` sent them in that
   order in every run at 150 % and 200 %, so there a filled surface never
   showed; sway did it for the second of two surfaces. **This stops every filled surface on a HiDPI output
   and is with the owner** (ticket 200's Blocked line).
2. An output removed is never dropped from `App::displays` (`client.rs` line
   1286, `GlobalRemove` is a TODO), and there is no event for an output
   added. `LayerSurfaces` polls the list every 500 ms and treats the
   compositor closing a surface as its output going away. Zed's `main` has
   both fixed.
3. On a compositor whose `wl_compositor` is below version 6 (`cosmic-comp`
   offers 5), entering an output overrides the fractional scale with the
   output's integer scale (`window.rs` lines 1191–1195), so at 150 % every
   GPUI window, layer or not, draws at 2 and is scaled down by the
   compositor: the right size, slightly soft.

### The nested test compositor

`packaging/tests/cosmic-nested.sh` runs System76's released `cosmic-comp`
without installing it: `fetch` checks `InRelease` against the one fingerprint
ADR 0038 names, then `Packages.gz` and the `.deb` against its SHA-256 sums, and
unpacks the `.deb` with `dpkg-deb -x` (every library it links resolves against
Zorin 18's). `start` runs it nested in a headless sway, both in one scope under
AGENTS.md's sandbox rules, and `stop` ends it by PID. Its limits:

- `cosmic-comp`'s nested backends (`winit`, `x11`) have exactly one output, so
  anything about several outputs runs against the headless sway itself
  (`start --sway-only`, `output add`, `output remove NAME`). Several outputs
  on `cosmic-comp` itself wait for ticket 201's VM.
- The scale is set through the `outputs.ron` it reads at start; the mode there
  must be the winit window's first size, 1280×800, or the whole entry is reset.
- `cosmic-comp` has no `wlr-screencopy` and no virtual pointer, so `shot` and
  `click` go through the outer sway, which shows `cosmic-comp`'s window 1:1;
  `keys` types on `cosmic-comp`'s own virtual keyboard.

```sh
cargo build -p layer-check                    # better-layer-check, better-wl-probe
packaging/tests/cosmic-nested.sh fetch        # once
packaging/tests/cosmic-nested.sh start --scale 2
packaging/tests/cosmic-nested.sh exec -- target/debug/better-layer-check --popup
packaging/tests/cosmic-nested.sh click 300 16 # logical pixels of the nested output
packaging/tests/cosmic-nested.sh keys 'ab{Return}'
packaging/tests/cosmic-nested.sh shot /tmp/shot.png
packaging/tests/cosmic-nested.sh stop
packaging/tests/session-layer-facts.sh OUT_DIR [CASE...]   # ticket 200's facts again
```

## Acceptance flows

Each runs in a disposable VM per base (Zorin OS 18 and Ubuntu 24.04), on
Better OS's built packages installed through Better Manager, never on the
owner's machine before release (AGENTS.md).

### Flow 1 — Install and switch (ticket 205)

1. Snapshot the person's home, `/etc`, `/usr`, `/var/lib/dpkg/status`.
2. In GNOME, Better Manager › Apps › Better OS session › Install; the
   confirmation lists the packages and their source and says the switch is at
   the next login; Log out now.
3. GDM preselects Better OS; log in. The bar, taskbar, desktop with
   `~/Desktop`'s files, wallpaper, appearance, scale, layouts, and Zhuyin are
   there. GDM is still `display-manager.service`.
4. `apt-cache policy` shows only the 14 names from System76's origin;
   `apt-get -s dist-upgrade` takes nothing from it.

### Flow 2 — A working day (ticket 222, then ticket 227 on the owner's machine)

Log in; Super+Space and type to launch; open Better Files and drag files;
Alt+Tab between three windows; the overview; switch workspaces with four
fingers (VM: a replayed libinput recording); type Zhuyin into a Better OS
window, Firefox, and an Electron application; open an application menu while
Fcitx 5 runs; join a Wi-Fi network that needs a password (the agent's prompt)
and pair a Bluetooth device where the VM has one; receive, act on, and clear
notifications; share the screen through the portal in a browser; play sound
and change volume from the bar and the keys; plug a USB drive and remove it;
lock with a composition open and unlock; let it idle to blank; suspend and
resume; an XEmbed tray icon from a Wine or GTK 2 program; Orca reads a Better
OS window; update a Better OS package and see every window run the new
program (AGENTS.md); log out.

### Flow 3 — Remove and return (ticket 205)

1. In the Better OS session, Remove: the next session is GNOME; log out; the
   purge runs.
2. Log in to GNOME. The snapshot comparison of flow 1 shows no difference
   outside GNOME's own logs and caches; GNOME's settings, extensions,
   keybindings, input sources, and taskbar are as before; `ls
   /etc/apt/sources.list.d /etc/apt/preferences.d` has nothing of System76's.
3. Repeat with "Also delete my settings and history" unticked: the session's
   `paths` stay, and a reinstall finds them.

## Ready to become the default

The session becomes the default (ticket 228) only when every item is true and
recorded:

1. Flows 1 and 3 pass on Zorin OS 18 and Ubuntu 24.04 (ticket 205).
2. Flow 2 passes in the VM with the released compositor (ticket 222) and again
   with Better OS's compositor (ticket 224).
3. The gesture patch is in (ticket 223) and every system gesture of S6 follows
   the fingers, reverses, and settles within the latency budget (ticket 224);
   the edge swipe works or the owner has deferred it (ticket 225).
4. Every surface is complete in the sense of ticket 76's direction 7: top bar
   (149, 152, 156–158, 160), taskbar (213), launcher (210), switcher (211),
   overview (212), notifications (215, 216), desktop and stacks (214, 144).
5. The owner has worked a week in it on the owner's machine after a release
   (ticket 227), with each fault found fixed or ticketed and none open that
   loses data or blocks work.
6. One COSMIC Epoch release has been taken through ticket 226's routine
   without breaking the session.
7. The owner has accepted ADR 0040.

## Constraints

- Ubuntu 24.04 bases only (Zorin OS 18, Ubuntu 24.04); amd64, and arm64 if
  ticket 201 finds System76's builds.
- Better OS's code is Rust (AGENTS.md). The session uses no GJS. COSMIC's
  programs are upstream packages taken as released, not first-party code.
- No privileged change outside `manager-daemon`; the third-party source is the
  one C2 allows, compiled into the daemon.
- Every sandbox desktop in a test runs under AGENTS.md's sandbox rules; a
  nested `cosmic-comp` counts as one (ticket 200).
- Nothing unreleased is installed on the owner's machine; every install check
  runs in a disposable VM.

## Assumptions and risks

- **Pop's binaries run on Zorin's libraries** — **unverified**; ticket 201. If
  not, the session needs Better OS to build the closure itself, which C2
  avoided: the line stops and goes back to the owner.
- **GDM starts the session** — **unverified**; ticket 201. If not, C3 goes back
  to the owner.
- **GPUI's layer-shell surface works on `cosmic-comp`** — seen working at
  100 % in the nested compositor (ticket 200); a surface that fills an axis is
  killed by a GPUI protocol error above 100 % ("Surfaces on layer-shell",
  defect 1). Until the owner decides how GPUI gets the fix, the top bar,
  taskbar, and desktop cannot be built for HiDPI; surfaces of a fixed size can.
  Several outputs on `cosmic-comp` itself are unseen until ticket 201's VM.
- **The gesture patch is feasible within budget** — ticket 202. If not, C8 and
  S6 go back to the owner before any surface is built to follow fingers.
- **Chinese input faults upstream** (issues 2947, 2952, 2702, 2778) —
  ticket 201 checks them before the session is offered.
- **A pin written wrong replaces the base system** — ticket 204 tests the pin
  against Pop's real `Packages` file and the host's policy.
- **COSMIC moves fast** (twelve releases in eleven months; the decoration
  default changed on 2026-11-06) — ticket 226's routine; Better OS windows ask
  for server-side decorations explicitly rather than relying on the default.
- **A session is a desktop's worth of small duties** — the process table and
  ticket 201's inventory; a gap becomes a ticket.
- **Every desktop-facing test doubles** while GNOME stays supported.

## Decisions

### Already decided by the owner (ADR 0038, 2026-11-21)
- C1: option D. C2: System76's source, key, and a pin limited to the closure.
  C3: GDM. C4: Fcitx 5 with Chewing. C5: the compositor's title bar. C6:
  Better Files draws the desktop. C7: does not arise. C8: released compositor
  first, the gesture patch once the rest works and upstream still has none.
  C9: COSMIC Settings at first.

### Decided here
- **The session's own dconf profile** (`betteros`), not GNOME's database and
  not COSMIC's name: GNOME stays untouched, and nothing confuses it with a
  stock COSMIC install.
- **Existing units bind through `graphical-session.target`** rather than gaining
  a second `WantedBy`: one unit, two sessions, backend chosen at run time.
- **`XDG_CURRENT_DESKTOP=BetterOS:COSMIC`** and session declarations name
  `betteros`.
- **AccountsService is called from `manager-core`**, not the daemon: it is the
  person's own account data under AccountsService's own polkit action.
- Rejected: reusing `cosmic-session` (fixed component list); a per-desktop
  `Condition` on every unit (two places decide the desktop).

### The owner's, still open
| # | Decision | Recommendation | Waits on it |
| --- | --- | --- | --- |
| S1 | Settings carried over one way, at install and at each Use Better OS, never written back to GNOME; or kept in step both ways | **One way**: GNOME stays exactly as left, which S3 requires; a change made in the session stays in the session | 208 |
| S2 | Removal pressed inside the session finishes after logout; or it is refused until the person is in GNOME | **Finish after logout**: one action, as AGENTS.md asks | 205 |
| S3 | What Show Applications and Super+A open, since GNOME's application grid is not in the session | **Better Launcher's All apps as a full-screen grid** on the overlay layer, the closest to the grid the owner likes | 210, 213 |
| S4 | How Better Input reaches applications in the session: (a) a Rust client of `zwp_input_method_v2` of its own, with X11 applications then needing an XIM server of Better OS's; (b) a Fcitx 5 addon, which is C++ and needs an ADR widening the language rule; (c) an out-of-process engine interface of Fcitx 5, if one exists | **Check (c) first** (ticket 221's first step); if none exists, (a) | 221 |
| S5 | The gesture latency budget: one frame at p95 | **Accept** | 202, 224 |
| S6 | Better OS's `cosmic-comp` cannot be restarted under a running session, so after its update the new compositor runs from the next login, said on the component page and the bar; an exception to AGENTS.md's "within a few seconds" update rule for this one package | **Accept the exception**, recorded in AGENTS.md with ticket 223 | 223 |

Ticket 161's N1 to N4 (the island, one list or two, the edge-swipe route, and
whether they exist without the bar) and Better Input's D1 to D9 stay open as
those documents list them.

## For the implementer

- May decide: crate names and internal structure, the exact unit sandbox keys,
  the strings of journal lines, the order of carry-over writes, the spring
  constants of a settle (one place, in `better-ui`).
- Must ask: anything that adds a package source, a package outside the
  closure, a new privileged method, a GJS file, a C or C++ file, or a change to
  the gesture protocol's rules above; any surface or setting that writes
  GNOME's configuration from the session.

## Sources

- [ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md) and its
  evidence in `/home/tim/Developer/better-os-evidence/143/`
  (`session-closure-on-host-noble.txt`, `wayland-info-and-portals.txt`,
  `session-and-config.txt`).
- `engineering/tickets/done/143-cosmic-desktop-evaluation.md`.
- `design/top-bar-spec.md` (sections 0, 6, 9.6, 11), ADR 0036.
- `design/better-input-spec.md`, ADR 0037.
- ADR 0012, 0015, 0018, 0030, 0033, 0035; AGENTS.md's user-unit, uninstall,
  update, and sandbox rules.
- [ADR 0039](/engineering/decisions/0039-compositor-fork-policy.md) and
  [ADR 0040](/engineering/decisions/0040-what-better-os-is.md) (proposed).
