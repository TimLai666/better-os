# ADR 0034: The keyboard's colour is written by the daemon's helper, never through a session ACL

## Status

Proposed on 2026-11-21 (ticket 136), revised the same day after its
security review (sandbox, device choice, the way back on `kbd_rgb_mode`,
level counts). Extends ADR 0029's helper-unit pattern
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
- **The card is for the laptop's own keyboard.** The daemon finds it
  itself: a built-in LampArray, meaning a hidraw node on the I2C bus whose
  sysfs `report_descriptor` (readable by everyone) declares one and that
  says it is a keyboard (`LampArrayKind` 1) or says nothing (no field, or
  0, undefined); otherwise asus-wmi's `kbd_rgb_mode` when that file exists.
  A LampArray on USB or Bluetooth (an external Dynamic Lighting keyboard, a
  mouse, a light strip) is out of scope: it is never read, never set, and
  never presented as the keyboard, even when it is the only LampArray. A
  built-in LampArray that names another kind (chassis, peripheral) is
  described for what it is and never driven. Nothing the caller sends
  becomes a path or a report byte other than the four channel values,
  which are scaled to the lamp's own level counts: a count N is off plus N
  levels, values 0 to N (HID Usage Tables 1.4, section 26.9; Microsoft's
  LampArray sample reports 0xFF and sends the 8-bit channel as it is).
- A LampArray command runs in a transient oneshot unit started with
  `systemd-run --wait --pipe`, in the sandbox every hardware helper shares
  (`manager_daemon::hardware::helper_sandbox`, ADR 0029 narrowed by this
  record's review): `/sys` an empty read-only tmpfs with `MountAPIVFS=no`;
  `PrivateDevices=yes`, so `/dev` holds the pseudo-devices and, bound in,
  this one node, which `DeviceAllow=/dev/hidrawN rw` is the only device
  the cgroup lets it open; `/proc/sys` and `/proc/sysrq-trigger` read-only;
  `ProtectSystem=strict`, `ProtectHome=`, `PrivateTmp=`, `PrivateNetwork=`,
  `ProtectKernelModules=`, `ProtectKernelLogs=`, `ProtectClock=`,
  `ProtectHostname=`, `NoNewPrivileges=`, `RestrictSUIDSGID=`,
  `RestrictRealtime=`, `RestrictNamespaces=`, `LockPersonality=`,
  `MemoryDenyWriteExecute=`, `SystemCallFilter=@system-service`,
  `RestrictAddressFamilies=AF_UNIX`, and `RuntimeMaxSec=10s`, so a hung
  I2C ioctl cannot hold the keyboard's lock or a removal's prerm. Its
  capability bounding set is `CAP_SETUID CAP_SETGID` and nothing else. It
  runs `better-manager-daemon --lamp-array hidrawN describe|set RRGGBB PCT|give-back`,
  which opens the node with `O_NOFOLLOW`, refuses anything that is not a
  character device answering `HIDIOCGRAWINFO`, reads the descriptor from
  the kernel (`HIDIOCGRDESC`, not sysfs, at most 4095 bytes as the kernel
  allows), and then drops to `nobody` (no groups, real, effective and saved
  IDs, which takes root's capabilities with it) before it reads a single
  feature report: everything the device answers is parsed by an
  unprivileged process holding one file descriptor. It refuses a
  descriptor that is not a LampArray and moves only the five LampArray
  feature reports it builds itself. It never calls `read(2)` on the node,
  so no input report (the airplane-mode button, report 0x5F) is ever seen,
  and it has no way to build report 0x5F or the 0xFF59 copy. If a colour
  fails after autonomous mode was turned off, it turns it back on once and
  reports the first error.
- Three hardening options are not used, on purpose: `ProtectKernelTunables=`,
  `ProtectControlGroups=`, and `ProtectProc=` each bring the API file
  systems back, and with them all of `/sys` over the one-file tmpfs. Seen
  with transient units in a user manager on systemd 255 (2026-11-21): with
  any one of them `ls /sys` listed 11 entries, without them 1; the same
  duplicate-mount rule ADR 0029 found for `ReadOnlyPaths=/sys`. Their
  protection is covered otherwise: `/proc/sys` is bound read-only (seen
  `ro` in that unit's mountinfo), the cgroup tree is under the empty
  `/sys`, and the process that reads the device is `nobody`.
- `kbd_rgb_mode` is written by ADR 0029's attribute helper, which now
  accepts that file under `/sys/devices/` with exactly a value the closed set
  produces (`cmd` 0: set for now, never saved to the firmware).
- **polkit:** one action, `org.betteros.manager.keyboard-lighting`,
  `allow_active=yes`, `auth_admin` for inactive and remote sessions. A colour
  is not worth a password at the desk, as UPower does not ask for the
  keyboard brightness; `KeyboardLighting` asks the same action, because a
  read opens the device.
- **A caller in a loop** is coalesced: calls wait their turn on an async
  lock (no thread each), only the newest choice waiting is written, at
  most one write every 100 ms, and past eight waiting a call is refused as
  busy, so the daemon still drains and leaves after an upgrade.
- **Kept and restored:** the daemon keeps the choice in
  `/var/lib/better-os/keyboard-lighting.json` with the keyboard it was made
  for (interface, and for a LampArray the HID bus, vendor, and product),
  written whole (synced temporary file, rename, synced directory).
  `better-hardware-restore.service` starts when either it or the charge
  limit's file exists and writes it back at boot through the same helper,
  on that keyboard only. It waits up to 15 s for an I2C keyboard that binds
  late; a keyboard still absent, a choice that no longer fits it, or a file
  that is empty or does not parse is a skip with a journal line, never a
  failed unit.
- **The way back is one action on both interfaces:** "the keyboard's own
  lighting". On a LampArray it turns autonomous mode on now. On
  `kbd_rgb_mode` no write brings the firmware's saved setting back before
  the next start (Better OS writes `cmd` 0, which never saved over it), so
  the choice is forgotten, nothing is written at boot, and the card says
  the keyboard's own setting returns at the next start (a marker under
  `/run/better-os` says so until then).
- **Removal:** the daemon package's prerm, and Better Manager's own prerm
  (it only recommends the daemon package, which can stay), run
  `--release-keyboard-lighting`: autonomous mode back on for the LampArray
  Better OS set (nothing is touched when nothing was set), and the choice
  and the marker deleted. `kbd_rgb_mode` needs no write: the firmware's own
  saved setting comes back at the next start.
- **Diagnostics:** the daemon logs each LampArray's description once, the
  first time it reads it (lamp count, kind, levels, programmable, update
  interval), the card's details row shows the same, and
  `better-manager-daemon --lamp-array-via-helper hidrawN describe` runs the
  helper as the daemon does and prints it.

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
  runs at a time, at most ten a second.
- A hostile local process running as the person at the desk can set the
  keyboard's colour or give it back to the firmware, at most ten times a
  second, which is what the page does, and nothing else: no other report,
  no other device, no read of the node's input. Another account or a
  remote session is asked for an administrator.
- A compromised helper (its input: feature reports from a hostile device
  posing as the built-in LampArray) is `nobody` with no capability, one
  hidraw descriptor, no network, no writable file system, a read-only
  `/proc/sys`, a system-call allow-list, and ten seconds. It cannot write
  `/proc/sys/kernel/core_pattern` or `kernel/modprobe`, create a node in
  `/dev`, or load a module. Before the drop, for the few system calls that
  open the node and read the descriptor, it is root without capabilities
  beyond `CAP_SETUID`/`CAP_SETGID`, inside the same sandbox, and has parsed
  nothing the device sent. The attribute helper for `kbd_rgb_mode` stays
  uid 0 with no capability at all: its only input is its two arguments,
  checked against closed sets, and the file it writes is root's.
- **Untested on a real system manager:** the sandbox properties above are
  asserted by tests as a list and were probed one by one in a user manager
  (systemd 255), but no transient unit with them has run as root against
  a real hidraw node or sysfs attribute. Ticket 136's owner check runs them
  for real; `packaging/hardware-unit/check.sh` runs the attribute helper's
  in a VM.
- Unknown until the owner checks on the real keyboard (ticket 136's notes):
  the lamp count, `LampArrayKind`, and per-lamp level counts; whether
  intensity and the WMI brightness levels 0–3 multiply, override each other,
  or are independent; whether the colour survives suspend and resume. That
  autonomous mode is a LampArray's state at power-on is the usage tables'
  ("Default value is enabled"), not observed on this keyboard.
- Another hidraw device, or another report of this one, means a new record.
