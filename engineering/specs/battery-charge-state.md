# What a battery icon shows

Ticket 170. Every battery icon and battery line Better OS draws follows one
rule, `hardware_core::charge_state`. Before it, each window had its own
mapping from the kernel's `status` to a picture and words, and every one drew
a battery held at its charge limit like a battery running the machine.

## The states

| State | When | Picture | zh-TW | English |
| --- | --- | --- | --- | --- |
| On battery | a battery is `Discharging` and no charger is online (or none is known), or every charger is offline | the level: full from 90%, medium from 40%, low below | 使用電池 | On battery |
| Plugged in, still draining | a battery is `Discharging` with a charger online (a charger too weak for the load) | battery with a plug and a caution mark | 已接上電源，仍在耗電 | Plugged in, still draining |
| Charging | a battery is `Charging` | battery with a bolt | 充電中 | Charging |
| Held at the limit | a charger is online and every battery rests inside its limit's window, a limit under 100% | battery, partly filled, with a plug | at the limit: 已接上電源，維持在 {limit}% / Plugged in, holding at {limit}%; under it: 已接上電源，維持在 {n}%（上限 {limit}%） / Plugged in, holding at {n}% (limit {limit}%); above it: 已接上電源，高於 {limit}% 上限，不充電 / Plugged in, not charging above the {limit}% limit | (same) |
| Plugged in and full | a charger is online and every battery is at the top of a battery with no limit or a limit of 100% (or `Full` below that window, the kernel's own idea of full) | full battery with a plug | 已接上電源，已充滿 | Plugged in, fully charged |
| Plugged in, not charging | a charger is online and a battery is `Not charging` below its window | battery with a plug and a caution mark | 已接上電源，沒有充電 | Plugged in, not charging |
| Unknown | any reading the decision needs is missing | empty battery | 無法判讀電池狀態 | Battery state unknown |

Each state also has short words for a narrow place (a list row, a card
header beside the charge figure): 使用電池 / On battery, 接電仍耗電 / Draining,
充電中 / Charging, 維持在上限 {limit}% / Held at {limit}% limit (高於上限 {limit}% /
Above {limit}% limit above it), 已充滿 / Fully charged, 沒有充電 / Not charging,
無法判讀 / Unknown.

No two states draw the same picture at the same charge; the two caution
states share theirs and differ in words. Nothing with a charger online draws a
level picture, so a laptop on its charger never looks like one on battery.
"Plugged in, not charging" below the limit is a charger that cannot supply
enough, a fault, or a pause the firmware chose. It is never called holding.

A time until empty or full is shown only while the charge moves (on battery,
draining, charging); held or full, a "not available" would read as a fault.

## Inputs

- **Charger online**: each other `power_supply`'s `type` and `online`. A
  supply counts as a charger when its type is Mains (an AC adapter), USB (a
  USB or USB-C port, a dock included) or Wireless (a charging pad), the types
  the kernel documents beside Battery and UPS
  (`Documentation/ABI/testing/sysfs-class-power`, `type`); kernels before 4.17
  spelled USB-C and USB charger kinds as types of their own (`USB_C`,
  `USB_PD`), so any type starting with `USB` counts. A UPS is a battery the
  kernel reports beside the machine's and does not count. Online when one
  counting supply says so, offline when every one says it is not, unknown when
  none counts or one could not be read. `hardware_core::charge_state::
  charger_online` is the one place this is decided; Better Awake's service,
  the System page and Better Monitor all ask it.
- **Each system battery** (a ThinkPad may have two; a mouse's is not one): its
  `status` (`Charging`, `Discharging`, `Full`, `Not charging`, `Unknown`), its
  `capacity`, its energy (or charge) now and full, and its own
  `charge_control_end_threshold` and `charge_control_start_threshold`. A
  battery without the end threshold has no limit and charges to 100%. One
  that has it and cannot be read is unknown, never "no limit".

Any battery charging is charging; any discharging is draining or on battery.
Otherwise the charger decides: offline means the battery runs the machine
whatever `status` says, unknown means nothing can be said. Online, each
battery is judged against its own window: one below it is plugged in and not
charging, one that cannot be judged makes the whole unknown, and all inside
their windows is held (at the highest limit among them) or full.

The charge of several batteries is weighted by energy when each reports it,
and their average otherwise.

## The limit's window

Firmware stops at the limit and starts again only after the charge has
fallen a little, so a held battery reads anywhere from the limit down to a
few points under it, and above it when the limit was lowered under the charge
(the firmware stops charging and does not drain).

The window's bottom is the battery's start threshold when that is a plausible
hold window: above 0, under the limit, and at most 20 points under it
(`START_WINDOW_MAX`). The tools that set both write them close together
(UPower and TLP default to 75 and 80; Dell's firmware asks for at least 5
points between them). ThinkPads ship with a start of 0, and a battery at 30%
that is not charging with its charger in is a fault, not a hold; so a start of
0, a start at or over the limit, or one more than 20 points under it is
ignored, and the bottom is five points under the limit (`HOLD_TOLERANCE`).
A person who sets a start 40 points under the limit sees "not charging"
between the two, which is true and is the price of telling a fault apart.

Five points is a recorded starting value, not a measured one. On 2026-11-21
the owner's machine (ASUS, `BAT1`, no start threshold) read `Not charging`
at exactly its limit, 80 of 80, with the adapter `ACAD` online and UPower
1.90.3 reporting `pending-charge`. How far it falls before charging again has
not been watched.

### Older kernels say `Unknown` at the limit

Before Linux 5.19 the ACPI battery driver reported `Unknown` for a battery
that was neither charging, discharging nor full, which is what a battery
stopped at its threshold is. Commit 185d20694a8a ("ACPI: battery: Make
"not-charging" the default on no charging or full info", 2022-04-27) made it
`Not charging`; it was merged for 5.19-rc1 (merge 4a577fca503a, pulled in
527953ef7125). Ubuntu 22.04's GA kernel is 5.15, so a supported machine can
show `Unknown` at its limit; its HWE kernels (5.19 onwards) cannot. Whether
Ubuntu backported the commit to its 5.15 has not been checked.

So the rule treats `Unknown` with a charger online like `Not charging` only
inside a known limit's window (held, or full at a limit of 100); anywhere
else, or with the limit unreadable, it stays unknown.

## Where it is drawn

`battery_look(readings, language)` returns the state, the picture
(`BatteryIcon`, whose file is in `better-ui`'s icon set) and the words, full
and short. Its callers:

- Better Monitor: the Overview's battery card (short words in its header, the
  time or the energy at its foot) and Performance list row (short words), the
  Battery page, the taskbar popup, and the taskbar item's menu line and
  tooltip (`monitor-tray`), which draws no battery picture (below). The "what is using the battery" section and the popup's
  battery list follow `ChargeState::on_battery`, not a metric of their own.
- Better Manager: Home's device banner, the System page's battery card and
  charge limit card, and the Keep awake page's charge card.
- Better Awake: the Battery section's power row (words only).
- The top bar's battery (ticket 157, `design/top-bar-spec.md` 6.5).

A window draws the picture with `Picture::from_path(look.icon.asset_path())`.
`crates/hardware-core/tests/one_battery_rule.rs` fails when a window maps the
kernel's status, Better Monitor's `battery.status` key, a battery level, a
power supply's type or a charger's state to a picture or words of its own, or
reads "on battery" from anything but the rule.

Better Awake's service reports only "on AC power", so the Awake window and the
Keep awake page read the batteries from sysfs themselves
(`hardware_platform::sysfs::charge_readings`) with each read of the service,
off the UI thread, and read again when the service signals or UPower's
`OnBattery` or display device changes (`hardware_platform::bus::
POWER_SUPPLY_CHANGES`), rather than widen the service's protocol. Without a
battery reading of their own, the rule gets only the charger the service saw.

## The taskbar item

Better Monitor's taskbar icon draws no battery (owner, 2026-11-22, ticket
295). The panel already has the desktop's own battery icon, and a second
beside it is a duplicate. So the icon offers no battery reading in either
style and "Show in the taskbar" has no battery check box; a choice stored
before that which showed the battery is rewritten without it the next time
the item starts (a choice of the battery alone falls back to the default
readings, so the icon never goes empty). The battery stays in the item's
menu and tooltip, and in the popup, as words: its charge and its state from
`battery_look`, and the time while the charge moves.

The desktop's battery is the one to change. Under GNOME it is the shell's,
and nothing outside the shell's code can change its picture (below). In the
Better OS session it is the top bar's status area (ticket 157), which draws
this rule's states; their pictures are designed there with the owner.

### The power mode beside it

The taskbar shows the power mode in force as a second item beside the icon,
drawn by the desktop from its icon theme (`power-profile-power-saver-symbolic`,
`power-profile-balanced-symbolic`, `power-profile-performance-symbolic`), so it
takes the panel's own colour in light and dark and is drawn at the panel's
own size. The mock-up that compared it with a mark inside the readings icon
is `better-os-evidence/295/mockup-power-mode.png`. How it looks is still to be
designed with the owner (ticket 295).

- The modes are the ones power-profiles-daemon lists (`Profiles`), read the
  way the System page reads them (`hardware_platform::bus::power_profiles`).
  Better OS never reads or writes `platform_profile` itself (ADR 0017); on a
  machine with `/sys/firmware/acpi/platform_profile_choices` the daemon's
  list is drawn from it.
- Only a daemon that is running is read: a name with no owner is absent and
  is never started through D-Bus activation, so a daemon someone stopped, or
  its package's removal stopped, stays stopped. A masked daemon is absent
  too.
- What is shown is one rule, `hardware_core::power::mode_shown`: the mode in
  force by name (`Profile::name`, the System page's words), the modes the
  daemon lists that this version knows, and the System page's caution
  sentences (`Caution::words`) when an application holds the mode or the
  daemon reduces it. A mode this version does not know, or one the daemon
  does not list, is not shown at all, never as the daemon's own word.
- The item is on the bus only while there is a mode to show, on a
  connection of its own: with no daemon, no mode to show, or the check box
  under "Show in the taskbar" unticked, it leaves the bus, which every
  watcher understands. A `Passive` status would not do: COSMIC's status area
  ignores it (`cosmic-applet-status-area`, read from source at
  `cb3ea91a`), and KDE keeps such an item in its overflow.
- A daemon that answers and cannot be read hides the item, and the menu and
  the popup say the mode could not be read. A watch on the daemon that ends
  (the system bus closed) or never starts says the same, so a stale mode is
  never left showing.
- The menu (both items share it) and the tooltips name the mode and its
  cautions; the popup lists every mode the machine offers with the one in
  force marked, and the cautions under them.
- It follows the daemon's `PropertiesChanged` and its name coming and going,
  so a mode set on the System page, by "Switch power mode with the charger",
  or by any other program shows at once.
- Whether the menu also switches the mode is the owner's to decide (ticket
  295); until then the menu only names it.

## The desktop's own battery icon (GNOME)

In the Better OS session the desktop's battery is the top bar's (ticket 157),
which draws this rule. While GNOME's own bar is the desktop's, its icon does
not show the held state on the supported releases, and nothing outside GNOME
Shell's code can make it. The evidence, read on the owner's machine on
2026-11-21 (Zorin OS 18, `gnome-shell 46.0-0ubuntu6~24.04.17+zorin1`,
`upower 1.90.3-1`):

- The icon is drawn by `PowerToggle._sync()` in
  `/org/gnome/shell/ui/status/system.js` inside
  `/usr/lib/gnome-shell/libshell-14.so`. It adds `-charging` only when UPower's
  `State` is `CHARGING`, uses `battery-level-100-charged-symbolic` only for
  `FULLY_CHARGED` (or charging at 100%), and otherwise names
  `battery-level-{10 × floor(percent / 10)}-symbolic`. A held battery is
  `PENDING_CHARGE` (5), so at 80% it is `battery-level-80-symbolic`: the icon
  a battery running the machine at 80% has. Zorin's taskbar extension has no
  battery code of its own; it shows this indicator.
- The shell's proxy reads only `Type`, `State`, `Percentage`, `TimeToEmpty`,
  `TimeToFull`, `IsPresent` and `IconName`
  (`org.freedesktop.UPower.Device.xml` in
  `gnome-shell-dbus-interfaces.gresource`). UPower's own `IconName`, which
  is `battery-full-charging-symbolic` here, is only the fallback when the
  theme lacks the computed name, and the Zorin themes have every
  `battery-level-*` icon.
- UPower 1.90.3 has no charge-threshold support: the battery's D-Bus
  properties list no `ChargeThreshold*` property and `upowerd` contains no
  "threshold" string. Ubuntu 22.04's UPower (0.99.17, the jammy package; not
  read on a 22.04 machine) is older still.
- GNOME Shell 42's `status/power.js` (Ubuntu 22.04) has the same rule
  (upstream tag 42.0, read from the GitHub mirror). 47.0 and 48.0 do too.
  49.0 adds `-plugged-in` for `PENDING_CHARGE` when UPower's
  `ChargeThresholdEnabled` is true, so a GNOME 49 desktop with a UPower that
  has the property draws the held state; whether UPower sets it for a limit
  written to sysfs directly, as Better OS's daemon does, has not been
  checked (UPower's source was not reachable from this machine).
- Nothing outside the shell's code decides the name. The state comes from the
  kernel's `status`, which Better OS will not misreport. An icon theme that
  redrew `battery-level-80-symbolic` would change it for every 80% battery,
  including one running the machine. Changing it needs code in the shell
  that decides something, which ADR 0012 and ADR 0018 do not allow, so the
  relay is not proposed while the GNOME bar (tickets 151 to 159) waits under
  ADR 0038's option D.

So the System page's charge limit card says 停在上限時，GNOME 本身的電池圖示看起來跟使用電池時一樣。
/ "At the limit, GNOME's own battery icon looks the same as on battery." when
it is true: on GNOME, while a limit under 100% is set, and when the shell is
older than 49 or UPower's display device has no `ChargeThresholdEnabled`
(`hardware_platform::bus::upower_reports_charge_threshold`, a read only).
Without either reading it says nothing. The sentence goes once the top bar
draws the desktop's battery.

## Done when

- `hardware-core`'s tests cover every state and the edges (the window's
  bottom, a plausible and an implausible start threshold, a limit lowered under
  the charge, `Full` at the limit, an older kernel's `Unknown`, two batteries,
  an offline charger, each missing reading), and that no two states share a
  picture at the same charge.
- Every place above shows the held state on the owner's machine, zh-TW and
  English, light and dark.
