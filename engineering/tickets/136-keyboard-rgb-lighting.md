# Ticket 136 — Keyboard backlight colour and effects

**Blocked by:** none

## Why

The owner asked on 2026-11-16 to set the keyboard backlight's RGB colour and
effects (breathing, colour cycle and so on) as ASUS's own software does.

## What was seen (read only, 2026-11-16)

- Machine: ASUS Vivobook S 14 M5406WA, kernel 7.0.
- The kernel exposes `/sys/class/leds/asus::kbd_backlight` with
  `max_brightness` 3 and no `multi_intensity`/`multi_index`; `asus-nb-wmi`
  has no `kbd_rgb_mode` or `kbd_rgb_state`. So the running driver offers
  brightness levels 0–3 only, not colour.

## What to build

- First establish whether this keyboard has RGB at all (ASUS's specification
  for this exact SKU, and its USB/HID or WMI interface). If it is white-only,
  the System page says so and this ticket closes with that evidence.
- If it has RGB: find the interface (asus-wmi `kbd_rgb_mode` on supported
  models, an ITE/Aura HID device, or a WMI method the kernel does not wrap),
  record the design in an ADR (privileged write through `manager-daemon`'s
  helper-unit pattern from ADR 0029, or a udev-granted HID node), and build
  colour, brightness and the effects the hardware supports (static,
  breathing, colour cycle, as the hardware offers — never simulated by a
  process rewriting colours many times a second), restored at boot.
- Models with the kernel's RGB support (`kbd_rgb_mode`) get the same page
  even if this one does not.

## Notes

**Blocked:** the check on the real keyboard needs the owner, on a released
build installed through Better Manager; everything else is built.

### What the keyboard is (read without privilege, 2026-11-21)

- It has RGB: ASUS lists every M5406 as "Backlit Chiclet Keyboard 1-Zone
  RGB" and FAQ 1051652 names the M5406WA a Dynamic Lighting model.
- The interface is a HID LampArray: `ITE5570:00 0B05:5570`, I2C,
  `hidraw1`, `hid-generic`. Its 748-byte report descriptor is saved as
  `crates/hardware-core/tests/fixtures/m5406wa-ite5570.rdesc` (and in the
  owner fixture tree under `crates/hardware-platform/tests/fixtures/owner`);
  parsing it gives feature reports 0x41 (23 bytes), 0x42 (3), 0x43 (29),
  0x44, 0x45 (10), 0x46 (2) on page 0x59, a vendor copy on 0xFF59, vendor
  report 0x5F, and the airplane-mode button (report 0x04).
- None of the kernel's interfaces: no `multi_intensity`, no `kbd_rgb_mode`
  (the firmware lacks WMI device 0x00100056/0x0010005A), no LampArray
  driver. ADR 0034 has the sources.

### What was built

- ADR 0034: written through `manager-daemon`'s helper unit, not a seat ACL
  (the node also carries vendor report 0x5F and input reports).
- `hardware_core::lamparray`: a bounded report-descriptor parser, the
  LampArray layout found by usage (any vendor, not this PID), describe
  (count, kind, interval, per-lamp levels, programmable), one colour and
  intensity for every lamp, autonomous mode back on. Tested against the real
  descriptor, every truncation of it, random and mutated bytes, and a fake
  device that answers short, for the wrong lamp, with no or 60,000 lamps.
- `hardware_core::lighting`: effects per interface (LampArray: steady colour
  and the keyboard's own lighting; `kbd_rgb_mode`: static, breathing, colour
  cycle, strobe as the driver and OpenRGB number them), the stored choice,
  the card's view.
- `hardware-platform`: detection by descriptor in sysfs, the client calls.
- `manager-daemon`: `KeyboardLighting`/`SetKeyboardLighting` on
  `Hardware1`, polkit `org.betteros.manager.keyboard-lighting`, the
  `--lamp-array` helper with hidraw ioctls, `kbd_rgb_mode` in the attribute
  helper, the boot restore, `--release-keyboard-lighting` in the prerm.
- The System page's Keyboard colour card (swatches, hue, saturation, colour
  brightness, effect, speed, one-colour, no-daemon, not-allowed and error
  states, zh-TW and English, a search entry).
- Packaging: the polkit action, the restore unit's second condition, the
  prerm and postrm, `verify-deb.sh` checks; the choice is named in
  `better-manager.yaml`'s `paths`.

### Not the brief's words, on purpose

- The choice is machine state kept by the daemon in
  `/var/lib/better-os/keyboard-lighting.json`, like the charge limit, not
  the person's home data: the boot restore runs as root before anyone logs
  in, and Better Manager deliberately owns no entry on the "Also delete my
  settings" list (`personal_data::nothing_on_the_list_is_better_manager_s_own_state`).
  Removing the daemon package gives the keyboard back to its firmware and
  deletes the file.
- The kernel's ABI document does not list `kbd_rgb_mode`'s modes; the
  numbers come from the driver's own clamp and OpenRGB.

### After the security review (2026-11-21)

- Both hardware helpers share one sandbox (`hardware::helper_sandbox`);
  the LampArray helper has a private `/dev` with its one node and drops to
  `nobody` before it reads the device. ADR 0034 and an amendment to ADR
  0029 say what holds and why three options are left out.
- The card is for the built-in keyboard only; a saved choice names its
  keyboard and is restored to it alone; a missing keyboard or a broken
  file is a skip, not a failed unit; the file is written durably.
- "The keyboard's own lighting" is offered on `kbd_rgb_mode` too.
- No false "one colour": no interface found, the keyboard's own report of
  no colour, and a lighting device of another kind are three states.
- Level counts follow HID Usage Tables 1.4 section 26.9 (N is off plus N
  levels, values 0 to N); a failed colour gives the lamps back.
- Calls are coalesced and rate-limited; an older daemon reads "needs
  updating"; the swatches are the kit's focusable, named `ColorSwatch`;
  the kit's settings cards keep a press on a header control from toggling
  the card (the three per-page workarounds are gone).
- Removing Better Manager gives the keyboard back too (it only recommends
  the daemon package, which may stay).

### The owner's check (on the next release, installed through Better Manager)

No unreleased build is run as root on the host (AGENTS.md). Everything
below goes through the released page and daemon, so it exercises the real
transient helper unit and its sandbox, which nothing else has run on a
real system manager (ADR 0034, Consequences).

1. **What the keyboard says about itself, without root.** Open System, then
   Keyboard colour, and read the **Device** row: device ID, zones (lamp
   count), kind, colour levels, brightness levels. The same line is in the
   journal the first time the daemon reads the keyboard:
   `journalctl -u better-manager-daemon -g 'keyboard lighting'`. If the
   card instead says the built-in lighting device "says it is a … not a
   keyboard", record that kind: the daemon does not drive it.
2. **Set a colour on the page** (Steady colour, then red, green, blue,
   white). The keyboard should change each time. Read the helper units'
   journal: `journalctl -t systemd -g 'Better OS keyboard lighting'` and
   `journalctl -b -g 'keyboard lighting'`; a failure names the property or
   system call that refused it (the sandbox list is in ADR 0034).
3. **Brightness levels and intensity:** with red at 100%, step the backlight
   through levels 0-3 (Fn key or the Keyboard backlight slider) and note
   what each does to the colour; then set Colour brightness to 50% and 0%
   (drawn only when the keyboard reports more than one brightness level) at
   level 3 and at level 1. Record whether they multiply, one overrides the
   other, or level 0 still turns it off.
4. **Restart** with a colour set: the colour should be back once the
   desktop is up (`journalctl -b -u better-hardware-restore` says
   `Written` or why it skipped).
5. **Suspend and resume** with a colour set: does it stay? (If not, a
   resume hook is owed.)
6. **"The keyboard's own lighting"** on the page: the firmware's lighting
   should return at once, and stay after a restart (nothing is restored).
7. **Remove** the `better-manager-daemon` package (or Better Manager): the
   keyboard should go back to its firmware lighting, and
   `/var/lib/better-os/keyboard-lighting.json` should be gone.

Each answer either confirms the follow-up in AGENTS.md or changes the
code: a level count read wrong, a kind the daemon refuses, a sandbox
property the real systemd rejects, a colour lost on resume.

Screenshots of the page states, taken in a headless sway sandbox against
`hardware_standin` (the real descriptor, a fake device behind it), are in
`/home/tim/Developer/better-os-evidence/136/`: the LampArray's own lighting,
steady colour (white, orange), `kbd_rgb_mode` not set, breathing and colour
cycle, one colour, no system service, and not allowed, in zh-TW and English,
light and dark. They show the stand-in's writes landing
(`0 1 0 160 255 0` in the fixture `kbd_rgb_mode`), not the real keyboard.
