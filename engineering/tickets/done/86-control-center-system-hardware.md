# Ticket 86 — Control center: System page and hardware controls

**Blocked by:** 84 for the page; the crate and daemon work can start at once

## Why

The owner wants power mode, battery charge limit, display brightness,
keyboard backlight, and fans in the control center, shown only where the
hardware has them, and every root write through `manager-daemon` and polkit
with a typed action per control (ADR 0017).

## What to build

Spec sections 4 and 11.

- **`hardware-core`** (new crate, no GPUI, no I/O): the typed model of what a
  machine has and what each card shows: power profiles and their holds and
  degradation, the battery and its health, charge-limit choices, brightness,
  keyboard levels, fan mode, temperature and fan readings with their labels
  and the one-reading-per-fan rule. Unit tests for every row of the spec's
  tables.
- **`hardware-platform`** (new crate): reads sysfs (`power_supply`, `leds`,
  `backlight`, `hwmon`, DMI) from a root that tests point at fixture trees,
  and talks to power-profiles-daemon (both bus names), gsd-power
  `Power.Screen` and `Power.Keyboard`, and UPower `KbdBacklight` over zbus.
  Fixture trees copy the owner's machine as read on 2026-11-12 (spec 11.1)
  plus a machine with nothing and an Intel `coretemp` machine.
- **Daemon.** `org.betteros.Hardware1` on `manager-daemon` with
  `SetChargeLimit(y)` and `SetFanMode(s)`, two polkit actions in
  `packaging/daemon/org.betteros.manager.policy` (English and zh_TW
  description and message), closed argument checks, device discovery inside
  the daemon, write then read back. Tests with a fake sysfs root and the fake
  authorizer, in the style of `crates/manager-daemon/tests/dbus_service.rs`.
  Update ADR 0007's consequences only by reference to ADR 0017.
- **Unit.** Keep `ProtectKernelTunables=yes`; add `ReadWritePaths=` for the
  two attributes. Show in a VM (Chefer AppCipe or a disposable VM, never the
  host) that the daemon can write a stand-in attribute under that
  configuration with systemd 255; if it cannot, stop and ask (ADR 0017).
- **Three facts to settle on the owner's machine, by the owner or with the
  owner present, each recorded in this ticket:** whether
  `charge_control_end_threshold` survives a reboot; that `pwm1_enable`
  accepts `0` for full speed and `2` for automatic on `asus-nb-wmi`; whether
  UPower's `KbdBacklight.SetBrightness` needs polkit on Ubuntu 24.04. Do not
  write to the host's sysfs from an agent session.
- **Restore at boot**, only if the charge limit does not persist:
  `/var/lib/better-os/hardware.json`, `better-hardware-restore.service`
  (oneshot, enabled by the daemon package's postinst), and
  `better-manager-daemon --restore-hardware`. Fan mode is never restored.
- **System page** in `manager-gui/src/pages/system/`: the Power, Display,
  Cooling, and About this PC groups, with every state of spec section 2.
  Temperatures refresh every 2 seconds only while the page is visible, off
  the UI thread. Brightness writes at most every 50 ms while dragging and
  follows the desktop's `PropertiesChanged`. Register its search entries and
  its Home items (battery health) and its Quick settings card (power mode).

## Acceptance

- [x] On the owner's machine every control in spec section 4 appears; on a
      fixture machine with none, the page shows only the empty sentence and
      About this PC.
- [x] A test for each control proves it is hidden when its hardware is
      absent and disabled with the reason when present but unreadable.
- [x] The daemon refuses any charge limit other than 60, 80, 100 and any fan
      mode other than `auto`, `full`, before touching a file, and refuses
      without polkit; tests show both.
- [x] Cancelling the polkit prompt leaves the value unchanged and shows the
      "authentication wasn't completed" bar.
- [x] The three machine facts are recorded here with how they were observed.
- [x] Light and dark, zh-TW and English, 1600 and 960, screenshotted (the
      sandbox reads the real sysfs and buses read-only; take no write action
      there).
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass. `Cargo.lock` gains only the two workspace
      crates.

## Power mode follows the charger (owner, 2026-11-13)

A switch on the System page, under Power mode: "Switch power mode with the
charger" / 「依充電狀態自動切換電源模式」. When on, plugging in sets
Performance and unplugging sets Power saver, through power-profiles-daemon
(`net.hadess.PowerProfiles`, no root), with the two target modes choosable
(defaults Performance and Power saver). It reacts to UPower's `OnBattery`
change, runs in the user session (a small part of an existing Better OS
session service, not a new daemon), does not fight a mode the person picks by
hand until the next plug event, and is off by default. Turning it off leaves
the current mode as it is. Tests use a fake UPower and a fake
power-profiles-daemon on a private bus.

## Notes

Delivered 2026-11-15 (branch `ticket-86`). One thing stays for the owner:
**does `charge_control_end_threshold` survive a reboot on the owner's
machine?** Set 80 on the System page, restart, and read
`/sys/class/power_supply/BAT1/charge_control_end_threshold` before
anything else runs, or with `systemctl mask better-hardware-restore.service`
for that one boot. The restore unit ships enabled either way (it writes only a
stored choice and nothing when the value already matches) and the card's
line is neutral: "Better OS sets it again each time the PC starts." If the
limit persists, the unit and its postinst lines can go.

**Machine facts, settled from source (no host write).**
- UPower's `KbdBacklight.SetBrightness` takes no polkit check: the handler,
  `up_kbd_backlight_set_brightness` in `src/up-kbd-backlight.c`
  (lines 177–197, identical at tags v0.99.17 for 22.04 and v1.90.3 for
  24.04), writes the LED with no authorization call, and the coordinator
  found no keyboard action in the host's
  `/usr/share/polkit-1/actions/org.freedesktop.upower.policy`. The page uses
  gsd-power first anyway.
- `pwm1_enable` on `asus-nb-wmi`: `pwm1_enable_store` in
  `drivers/platform/x86/asus-wmi.c` (v5.15 lines 1758–1800, v6.8 lines
  2432–2480) takes 0 (`ASUS_FAN_CTRL_FULLSPEED`) and 2 (`ASUS_FAN_CTRL_AUTO`)
  for a `FAN_TYPE_SPEC83` fan and refuses anything else with `-EINVAL`. The
  owner's machine has `pwm1_enable` and no `pwm1`, and the driver shows
  `pwm1` only for `FAN_TYPE_AGFN` (v6.8 line 2700, v5.15 line 1861), so this
  fan is SPEC83 and 0/2 are its two values.
- Charge-limit persistence: above, for the owner.

**Not offered, from source.** `charge_mode` is `DEVICE_ATTR_RO` (v6.8 lines
632–645; it reads `10` on the owner's machine): there is nothing to set.
`throttle_thermal_policy` is the backend asus-wmi registers as the
`platform_profile` handler that power-profiles-daemon drives (v6.8 line 35,
store at 3495); writing it beside the daemon would fight Power mode, which is
the spec's own reason for not writing `platform_profile` directly.

**The `/sys` narrowing (owner decision 2026-11-15, ADR 0029, built).**
`ProtectKernelTunables=yes` leaves `/sys` writable on Ubuntu 24.04's systemd
255 (the read-only entry is dropped as a duplicate of the API mounts; so is
`ReadOnlyPaths=/sys`, and `ProtectControlGroups=yes` brings the same mounts
in). Both units now have `BindReadOnlyPaths=/sys` plus
`ProtectKernelTunables=yes`, nothing opened under `/sys`; each write is a
transient helper unit (`TemporaryFileSystem=/sys:ro`, `BindPaths=` of the one
file, no capabilities) running `better-manager-daemon --write-attribute`,
which re-checks file and value. `packaging/hardware-unit/check.sh`, in the VM
(cloud image, systemd 255.4-1ubuntu8.18, kernel 6.8.0-147) and in a
container, all PASS: restore 100 → 80 through the helper; the daemon's
sandbox cannot write the fan switch, a real sysfs file
(`transparent_hugepage/enabled`), or `/proc/sys`, and the helper's own code
cannot write from there; the daemon's sandbox → helper writes `pwm1_enable`
2 → 0; the restore's sandbox cannot write `/sys`; the helper cannot write
another sysfs file, another attribute, or create a file under `/sys`; the
helper refuses any file but the two. (The first VM run of this check hung
after the fifth step and was cut off by the run's timeout; the rerun with a
per-step timeout passed every step. No cause was found.)

**Built.**
- `hardware-core`: every card rule, unit-tested.
- `hardware-platform`: sysfs under a root (fixtures `owner`, `nothing`,
  `intel`), hwmon through `monitor-collectors-linux` (feature `sensors`),
  PPD (both names) / gsd-power / UPower / GNOME Shell clients, the
  `Hardware1` client, the charger-switch store, `test-support` stand-ins.
- `manager-daemon`: `org.betteros.Hardware1`, argument before polkit, a
  dismissed prompt told apart, discovery inside the daemon, the helper
  writer and `--write-attribute`, `--restore-hardware`; tests over a private
  bus with a fake root and fake authorizers; `tests/unit_paths.rs`.
- Packaging: two polkit actions (en, zh_TW), bus policy, the units above,
  `better-hardware-restore.service` enabled by the postinst, disabled at
  removal, purged; `verify-deb.sh` checks for all of it.
- System page, Home's battery item and Power mode card, search entries; the
  charger switch in `better-window-attention`; kit additions
  `SettingsCard::caution`, `words::celsius`, `words::rpm`.

**Deviations.**
- The combo box's administrator-password note shows from the start, not only
  once it opens: the kit's `ComboBox` reports no open state.
- A labelled fan is "CPU fan" only when its label contains "cpu"; other
  unlisted sensors show the hwmon name with the driver's label.

**Checks run.** See the commit message of the delivering commit for the list;
screenshots under `.shots/` in the worktree (headless GNOME Shell 46, real
sysfs and system bus read-only, writes only to stand-ins on private buses).
