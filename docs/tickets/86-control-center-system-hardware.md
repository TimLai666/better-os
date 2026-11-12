# Ticket 86 — Control center: System page and hardware controls

**Blocked by:** 84 for the page; the crate and daemon work can start at once ·
**Status:** ready

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

- [ ] On the owner's machine every control in spec section 4 appears; on a
      fixture machine with none, the page shows only the empty sentence and
      About this PC.
- [ ] A test for each control proves it is hidden when its hardware is
      absent and disabled with the reason when present but unreadable.
- [ ] The daemon refuses any charge limit other than 60, 80, 100 and any fan
      mode other than `auto`, `full`, before touching a file, and refuses
      without polkit; tests show both.
- [ ] Cancelling the polkit prompt leaves the value unchanged and shows the
      "authentication wasn't completed" bar.
- [ ] The three machine facts are recorded here with how they were observed.
- [ ] Light and dark, zh-TW and English, 1600 and 960, screenshotted (the
      sandbox reads the real sysfs and buses read-only; take no write action
      there).
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
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
