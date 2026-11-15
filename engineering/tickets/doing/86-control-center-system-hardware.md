# Ticket 86 — Control center: System page and hardware controls

**Blocked by:** 84 for the page; the crate and daemon work can start at once
**Blocked:** the three machine facts need the owner at the owner's machine
(no agent may write the host's sysfs), and the owner has to decide what to do
about `/sys` not being read-only under `ProtectKernelTunables=yes` on systemd
255 (Notes). Everything else is built.

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
- [ ] The three machine facts are recorded here with how they were observed.
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

State on 2026-11-15 (branch `ticket-86`).

**Built.**
- `crates/hardware-core`: every card rule (power profiles, holds and
  degradation, battery health and rows, charge-limit choices and the closed
  set, keyboard levels for gsd-power and UPower, the 50 ms brightness
  throttle, fan modes and `pwm1_enable`, sensor labels and the
  one-reading-per-fan rule, which groups are drawn, write outcomes, and the
  charger switch's decision). 38 unit tests.
- `crates/hardware-platform`: sysfs under a root (fixtures `owner` from the
  machine as read on 2026-11-12 and 2026-11-15, `nothing`, `intel`), hwmon
  through `monitor-collectors-linux` behind a `sensors` feature, the
  PPD (both names) / gsd-power / UPower / GNOME Shell clients, the
  `Hardware1` client, the charger-switch store, and `test-support` stand-ins
  for every service. Bus tests run against the stand-ins on a private bus.
- `manager-daemon`: `org.betteros.Hardware1` (`hardware.rs`) on the same name
  and path, argument checked before polkit, own polkit action per method,
  a dismissed prompt told apart (`polkit.dismissed`, `daemon.error.auth_cancelled`),
  discovery inside the daemon, write then read back; `--restore-hardware`;
  `tests/hardware.rs` over a private bus with fake sysfs and the fake
  authorizers; `tests/unit_paths.rs` holds the unit's `ReadWritePaths=` to
  `BATTERY_NAMES` and `FAN_PLATFORM_DEVICES`.
- Packaging: two polkit actions (en and zh_TW), the bus policy for the new
  interface, `ReadWritePaths=` on the daemon unit,
  `better-hardware-restore.service` (enabled by the postinst, disabled at
  removal, purged), `verify-deb.sh` checks for all of it.
- System page `crates/manager-gui/src/pages/system/`: every state of spec
  section 2, the four groups, the charger switch with its two modes,
  temperatures every 2 s only while shown (off the UI thread), brightness at
  most every 50 ms while dragging and following `PropertiesChanged`, search
  entries for every card, Home's worn-battery item, Home's Power mode card.
  Home's battery now reads through `hardware-platform`, one rule for both.
- The charger switch lives in `better-window-attention` (`charger.rs`), with
  fake UPower and PPD tests on a private bus.
- The kit: `SettingsCard::caution` (the caution line) and `words::celsius`,
  `words::rpm`.

**The unit question (ADR 0017's VM check).** `packaging/hardware-unit/run.sh`
and the same `check.sh` in a disposable Ubuntu 24.04 cloud VM (qemu/KVM,
systemd 255.4-1ubuntu8.18, kernel 6.8.0-147): the restore unit wrote 100 → 80
and the daemon unit's own `[Service]` wrote `pwm1_enable` 2 → 0, both on
tmpfs stand-ins. So the daemon can write. But `ProtectKernelTunables=yes`
leaves `/sys` writable on that systemd: without `ReadWritePaths=` the write
also went through, a path outside them was writable, and a real sysfs file
(`/sys/kernel/mm/transparent_hugepage/enabled`) was written by a unit with
`ProtectKernelTunables=yes` and also with `ReadOnlyPaths=/sys`. systemd's
debug log: `/run/systemd/mount-rootfs/sys (read-only) is duplicate.` (the
read-only entry loses to the API VFS bind the option implies). The ticket
says to stop and ask only if the daemon cannot write; it can, so nothing was
dropped, but the protection ADR 0017 assumed does not exist on 24.04.
**Owner decision:** accept it (the lines stay, inert on 255), or find another
narrowing (a separate helper with a mount namespace of its own, an
`InaccessiblePaths=` list, `SystemCallFilter=`), which would be a new ADR.

**Machine facts, not settled (owner, at the machine).** None was observed;
no agent wrote the host's sysfs.
1. Does `charge_control_end_threshold` survive a reboot? Set 80 in the
   System page, reboot, `cat /sys/class/power_supply/BAT1/charge_control_end_threshold`.
   The restore unit ships enabled either way (it writes only a stored choice
   and does nothing when the value already matches);
   `hardware_core::charge::PERSISTS_ACROSS_RESTART` is `None`, so the card
   says nothing about restarts until this is answered. If it persists,
   drop the unit and its postinst lines.
2. Does `asus-nb-wmi` take `0` (full) and `2` (auto) on `pwm1_enable`? Pick
   Full speed and Automatic on the page; the daemon reads back and the page
   shows a "reads back as" bar if not.
3. Does UPower's `KbdBacklight.SetBrightness` need polkit on 24.04? Only
   matters where gsd-power has no keyboard (the owner's machine has it).
   On the owner's machine: `busctl call org.freedesktop.UPower
   /org/freedesktop/UPower/KbdBacklight org.freedesktop.UPower.KbdBacklight
   SetBrightness i 1` as the person.

**Not done, and why.**
- The combo box's authentication note shows in the description from the
  start, not "once the combo box opens": the kit's `ComboBox` reports no
  open state. Adding one is a kit change (spec 13) left for whoever needs it
  next.
- `ReadWritePaths=` cannot name a battery by glob, so the unit names
  `BAT0`, `BAT1`, `BAT2`, `BATT`; a battery under another name is shown and
  the daemon reports it absent for writing.
- A labelled fan whose label does not contain "cpu" is shown by its label,
  not as "CPU fan" (the spec's "any `fanN_label`" would call a GPU fan a CPU
  fan). Other unlisted sensors show the hwmon name and the driver's label.

**Checks run.** `cargo fmt --all --check`; `cargo +1.99.0 clippy --workspace
--all-targets --all-features -D warnings`; `cargo +stable` (1.97.1) clippy the
same; `cargo +1.95 check --workspace --all-targets --locked`; `cargo test
--workspace`; `packaging/tests/maintainer-scripts.test.sh`;
`generate-third-party-notices.sh --check`; `build-deb.sh` for 24.04 and
`verify-deb.sh`; the unit check in a container and in the VM. Screenshots
(headless GNOME Shell 46, private session bus and HOME, real sysfs and the
real system bus read-only; writes only to stand-ins on private buses):
`.shots/` in the worktree, every variant of en/zh, light/dark, 1600/960,
plus the cancelled, not-allowed, failed, lap/hold, empty-machine, and Home
states.
