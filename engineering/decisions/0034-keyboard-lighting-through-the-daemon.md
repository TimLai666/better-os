# ADR 0034: The keyboard's colour is written by the daemon's helper, never through a session ACL

## Status

Proposed on 2026-11-21 (ticket 136). Extends ADR 0029's helper-unit pattern
from sysfs attributes to one HID device node; ADR 0007's protocol and ADR
0029's attribute helper are unchanged except that `kbd_rgb_mode` joins the
attribute helper's closed set.

## Context

The owner asked for the keyboard backlight's colour and effects, as ASUS's
own software sets them (ticket 136). On the owner's ASUS Vivobook S 14
M5406WA (BIOS M5406WA.314, kernel 7.0.0-43), read without privilege on
2026-11-21:

- ASUS's specification lists every M5406 keyboard as "Backlit Chiclet
  Keyboard 1-Zone RGB"
  (https://www.asus.com/laptops/for-home/vivobook/asus-vivobook-s-14-oled-m5406/techspec/),
  and ASUS FAQ 1051652 names the M5406WA as a Windows Dynamic Lighting model
  (https://www.asus.com/sg/support/FAQ/1051652).
- The kernel's keyboard interfaces offer no colour: `asus::kbd_backlight`
  has `max_brightness` 3 and no `multi_intensity`; asus-wmi creates
  `kbd_rgb_mode` only when the firmware reports WMI device 0x00100056 or
  0x0010005A (`asus_wmi_dev_is_present`, `drivers/platform/x86/asus-wmi.c`),
  and this firmware reports neither, so the file is absent; hid-asus does not
  list product 0x5570; there is no LampArray driver in the kernel (a
  `hid-lamparray` series was at v4, unmerged, in March 2026:
  https://lkml.iu.edu/hypermail/linux/kernel/2603.3/03294.html).
- The colour is a HID LampArray: `/sys/class/hidraw/hidraw1` is
  `ITE5570:00 0B05:5570` on I2C, bound to `hid-generic`, `/dev/hidraw1` is
  `root:root 0600`. Its 748-byte report descriptor (saved as
  `crates/hardware-core/tests/fixtures/m5406wa-ite5570.rdesc`, sha256
  `fa58cf01…18bb0`) holds, in this order: a Generic Desktop Wireless Radio
  Controls collection (report 0x04: the airplane-mode button's input and
  its LED's output), a vendor collection on page 0xFF30 (report 0x5F, 50
  bytes as input, output, and feature, protocol unknown), the LampArray
  application collection on the Lighting and Illumination page 0x59
  (feature reports 0x41 LampArrayAttributes, 0x42 LampAttributesRequest,
  0x43 LampAttributesResponse, 0x44 LampMultiUpdate, 0x45 LampRangeUpdate,
  0x46 LampArrayControl), and a copy of the LampArray reports on vendor page
  0xFF59 (0x31–0x36).
- The LampArray usages (HID Usage Tables, page 0x59) define colour,
  intensity, and autonomous mode, and no effect: Windows draws its Dynamic
  Lighting effects by sending colours from the host many times a second.

So the M5406WA can show one colour and intensity for its one zone, or its
firmware's own lighting. Breathing or a colour cycle on it would be Better
OS rewriting colours on a timer, which the ticket forbids.

The other ASUS interface, asus-wmi's `kbd_rgb_mode` on TUF and ROG models
(`/sys/class/leds/asus::kbd_backlight/kbd_rgb_mode`, write-only, root
`0200`), takes `cmd mode red green blue speed`, and the firmware runs the
effect. The kernel's ABI document (`Documentation/ABI/testing/
sysfs-platform-asus-wmi`) does not list it; the driver forces mode 9 and
anything from 12 up to 10, and OpenRGB's ASUS TUF controller names 0 static,
1 breathing, 2 colour cycle, 10 strobing
(`Controllers/AsusTUFLaptopController/AsusTUFLaptopController_Windows.h`).

Writing a LampArray means sending feature reports to its hidraw node, which
only root can open. Two ways were open.

## Decision

- **The daemon writes, through a helper unit that can open one node.**
  `org.betteros.Hardware1` gains `KeyboardLighting() → s` and
  `SetKeyboardLighting(s effect, y red, y green, y blue, y intensity,
  y speed) → s`. The effect is one of `firmware`, `static`, `breathing`,
  `cycle`, `strobe`, checked against what the interface offers before
  anything is touched (a LampArray takes `static` and `firmware` only);
  intensity is 0–100, speed 0–2. Neither text argument is echoed.
- The daemon finds the keyboard itself: the hidraw nodes whose sysfs
  `report_descriptor` (readable by everyone) declares a LampArray, I2C ones
  first, the first that says it is a keyboard (`LampArrayKind` 1), or says
  nothing and sits on I2C. A LampArray mouse or light strip is never taken.
  Without one it uses `kbd_rgb_mode` when that file exists. Nothing the
  caller sends becomes a path or a report byte other than the four channel
  values, which are scaled to the lamp's own level counts.
- A LampArray command runs in a transient oneshot unit started with
  `systemd-run --wait --pipe`: `DevicePolicy=closed` with exactly
  `DeviceAllow=/dev/hidrawN rw`, `/sys` an empty read-only tmpfs,
  `MountAPIVFS=no`, `ProtectSystem=strict`, `ProtectHome=yes`,
  `PrivateTmp=yes`, `PrivateNetwork=yes`, `NoNewPrivileges=yes`, an empty
  `CapabilityBoundingSet=`, `RestrictAddressFamilies=AF_UNIX`. It runs
  `better-manager-daemon --lamp-array hidrawN describe|set RRGGBB PCT|give-back`,
  which opens the node with `O_NOFOLLOW`, refuses anything that is not a
  character device answering `HIDIOCGRAWINFO`, reads the descriptor from
  the kernel (`HIDIOCGRDESC`, not sysfs), refuses one that is not a LampArray,
  and moves only the five LampArray feature reports it builds itself. It
  never calls `read(2)` on the node, so no input report (the airplane-mode
  button, report 0x5F) is ever seen, and it has no way to build report 0x5F
  or the 0xFF59 copy.
- `kbd_rgb_mode` is written by ADR 0029's attribute helper, which now
  accepts that file under `/sys/devices/` with exactly a value the closed set
  produces (`cmd` 0: set for now, never saved to the firmware).
- **polkit:** one action, `org.betteros.manager.keyboard-lighting`,
  `allow_active=yes`, `auth_admin` for inactive and remote sessions. A colour
  is not worth a password at the desk, as UPower does not ask for the
  keyboard brightness; `KeyboardLighting` asks the same action, because a
  read opens the device.
- **Kept and restored:** the daemon keeps the choice in
  `/var/lib/better-os/keyboard-lighting.json` (`deny_unknown_fields`), and
  `better-hardware-restore.service` now starts when either it or the charge
  limit's file exists and writes it back at boot through the same helper,
  waiting up to 15 s for an I2C keyboard that binds late. The keyboard's own
  default is no choice: the file is deleted and nothing is written at boot.
- **Removal:** the daemon package's prerm runs
  `--release-keyboard-lighting`, which turns autonomous mode back on for a
  keyboard Better OS set (nothing is touched when nothing was set), and the
  postrm deletes the file. `kbd_rgb_mode` needs nothing: the firmware's own
  saved setting comes back at the next start.

## Not chosen

- **A seat ACL on the one hidraw node** (`SUBSYSTEM=="hidraw",
  KERNELS=="0018:0B05:5570.*", TAG+="uaccess"` in a file before
  `73-seat-late.rules`). It needs no daemon, and it would have let the window
  open the node itself. It was not chosen because the node is not a
  lighting-only device:
  - An ACL grants the whole node. Any program the person runs, and anything
    that compromises one, could then write any report the device takes:
    vendor report 0x5F, whose protocol nobody outside ASUS and ITE has
    documented, is a 50-byte feature and output report on an ITE embedded
    controller. Vendor reports on such controllers are where firmware update
    and configuration commands usually sit; nothing shows that 0x5F is
    harmless, and a write there could leave the keyboard controller in a
    state only ASUS's tools recover.
  - The same ACL lets the session read the node's input reports: the
    airplane-mode button and report 0x5F's input form, again of unknown
    content. Whether it carries key events is not known.
  - A rule by product ID covers only this keyboard; a rule general enough
    for other LampArray keyboards would match by property, and udev cannot
    read a report descriptor, so it would grant every hidraw node a matching
    rule names (other vendors' keyboards with their own vendor reports).
  - The restore at boot would still need root, because an ACL exists only
    for the active seat's session.
- **asusctl or OpenRGB as a dependency:** asusctl has no entry for this
  device (asusctl issue 670), and both are large privileged daemons with
  their own effect engines; OpenRGB's generic LampArray controller would
  also need the node opened to the session.
- **Effects drawn by Better OS:** forbidden by the ticket, and the reason no
  effect is offered on a LampArray. The page says why.
- **The vendor report 0x5F:** its protocol is unknown and the effects it
  might carry are unverified. It is never written.

## Consequences

- Each change starts one transient unit; a description is read once per
  node and descriptor and kept while the daemon runs. One keyboard change
  runs at a time.
- A hostile local process running as the person at the desk can set the
  keyboard's colour or give it back to the firmware, which is what the page
  does, and nothing else: no other report, no other device, no read of the
  node's input. Another account or a remote session is asked for an
  administrator. A compromised helper holds one hidraw node of the keyboard
  for the length of one command, without capabilities or network.
- Unknown until the owner checks on the real keyboard (ticket 136's notes):
  the lamp count, `LampArrayKind`, and per-lamp level counts; whether
  intensity and the WMI brightness levels 0–3 multiply, override each other,
  or are independent; whether the colour survives suspend and resume; and
  whether autonomous mode is what the keyboard shows at power-on. A level
  count of N is taken to mean values 0 to N−1, per the usage tables' "number
  of levels"; that is read, not observed.
- Another hidraw device, or another report of this one, means a new record.
