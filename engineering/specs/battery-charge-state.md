# What a battery icon shows

Ticket 170. Every battery icon and battery line Better OS draws follows one
rule, `hardware_core::charge_state`. Before it, each window had its own
mapping from the kernel's `status` to a picture and words, and every one drew
a battery held at its charge limit like a battery running the machine.

## The states

| State | When | Picture | zh-TW | English |
| --- | --- | --- | --- | --- |
| On battery | `status` is `Discharging`, or no charger is online | the level: full from 90%, medium from 40%, low below | 使用電池 | On battery |
| Charging | `status` is `Charging` | battery with a bolt | 充電中 | Charging |
| Held at the limit | a charger is online, `status` is `Not charging` or `Full`, the limit is under 100%, and the charge is at or near the limit | battery, partly filled, with a plug | 已接上電源，維持在 {limit}% | Plugged in, holding at {limit}% |
| Plugged in and full | a charger is online, `status` is `Not charging` or `Full`, and the charge is at the top of a battery with no limit or a limit of 100% (or `Full` below that window, which is the kernel's own idea of full) | full battery with a plug | 已接上電源，已充滿 | Plugged in, fully charged |
| Plugged in, not charging | a charger is online, `status` is `Not charging`, and the charge is below the limit's window | the level | 已接上電源，沒有充電 | Plugged in, not charging |
| Unknown | any reading the decision needs is missing | empty battery | 無法判讀電池狀態 | Battery state unknown |

"Plugged in, not charging" below the limit is a charger that cannot supply
enough, a fault, or a pause the firmware chose. It is never called holding,
and it keeps the level picture: a plug would read as "all is well".

## Inputs

- **Charger online**: the `online` of every `power_supply` that is not a
  battery. Online when one says so, offline when every one says it is not,
  unknown when there is none or one could not be read.
- **Status**: the system battery's `status` (`Charging`, `Discharging`,
  `Full`, `Not charging`, anything else is unknown).
- **Charge**: `capacity`, 0 to 100.
- **Limit**: `charge_control_end_threshold`, whether Better OS set it or the
  firmware reports it, and `charge_control_start_threshold` when the battery
  has one. A battery without the end threshold has no limit and charges to
  100%. One that has it and cannot be read is unknown, never "no limit".

`Charging` and `Discharging` are the kernel's own answer and need nothing
else. Otherwise the charger decides: offline means the battery runs the
machine whatever `status` says, unknown means nothing can be said.

## The limit's window

Firmware stops at the limit and starts again only after the charge has
fallen a little, so a held battery reads anywhere from the limit down to a
few points under it. The window is from the start threshold, when the battery
reports one below the limit, or from five points under the limit
(`HOLD_TOLERANCE`), up to 100%. A charge above the limit (a limit lowered
under the charge) is held too.

Five points is a recorded starting value, not a measured one. On 2026-11-21
the owner's machine (ASUS, `BAT1`, no start threshold) read `Not charging`
at exactly its limit, 80 of 80, with the adapter `ACAD` online and UPower
1.90.3 reporting `pending-charge`. How far it falls before charging again has
not been watched.

## Where it is drawn

`battery_look(readings, language)` returns the state, the picture
(`BatteryIcon`, whose file is in `better-ui`'s icon set) and the words. Its
callers:

- Better Monitor: the Overview's battery card and list row, the Battery page,
  and the taskbar popup (`monitor-gui`), and the taskbar item's menu line
  (`monitor-tray`, words only: its icon is a level bar, not a battery).
- Better Manager: Home's device banner, the System page's battery card and
  charge limit card, and the Keep awake page's charge card.
- Better Awake: the Battery section's power row (words only).
- The top bar's battery (ticket 157, `design/top-bar-spec.md` 6.5).

A window draws the picture with `Picture::from_path(look.icon.asset_path())`.
`crates/hardware-core/tests/one_battery_rule.rs` fails when a window maps
the kernel's status, Better Monitor's `battery.status` key, or a battery level
to a picture or words of its own.

Better Awake's service reports only "on AC power", so the Awake window and
the Keep awake page read the battery from sysfs themselves
(`hardware_platform::sysfs::charge_readings`) each time the service's status
arrives, rather than widen the service's protocol.

## The desktop's own battery icon (GNOME)

In the Better OS session the desktop's battery is the top bar's (ticket 157),
which draws this rule. While GNOME's own bar is the desktop's, its icon does
not show the held state, and nothing outside GNOME Shell's code can make it.
The evidence, read on the owner's machine on 2026-11-21 (Zorin OS 18,
`gnome-shell 46.0-0ubuntu6~24.04.17+zorin1`, `upower 1.90.3-1`):

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
  read on a 22.04 machine) is older still. Neither supported release's UPower
  can tell the shell a limit holds the battery.
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

So the System page's charge limit card says, on GNOME and while a limit under
100% is set: 「電池停在上限時，桌面本身的電池圖示看起來會跟使用電池時一樣，沒有充電符號：這是 GNOME 自己的畫法，GNOME 以外的程式改不了。」 / "At the limit, the
desktop's own battery icon looks the same as on battery, with no charging
mark: GNOME draws it that way, and nothing outside GNOME can change it." The
sentence goes once the top bar draws the desktop's battery.

## Done when

- `hardware-core`'s tests cover every state and the edges (the window's
  bottom, a start threshold, a limit lowered under the charge, `Full` at the
  limit, an offline charger, each missing reading).
- Every place above shows the held state on the owner's machine, zh-TW and
  English, light and dark.
