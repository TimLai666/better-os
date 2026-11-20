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

**Blocked:** the check on the real keyboard needs the owner (root, or the
released daemon); everything else is built.

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

### The owner's check (needs root; on a build of this branch)

1. Read the keyboard: `sudo target/debug/better-manager-daemon --lamp-array
   hidraw1 describe`. Record `lamps`, `kind`, `min_update_interval_us`, and
   `levels` (red, green, blue, intensity). `kind` should be `keyboard`; if
   it is `undefined` the daemon still takes it (I2C), anything else and the
   page shows one colour.
2. Set a colour: `... --lamp-array hidraw1 set ff0000 100`. The keyboard
   should go red. Then `set 00ff00 100`, `set 0000ff 100`, `set ffffff 100`.
3. Brightness levels and intensity: with red at 100, step the backlight
   through levels 0-3 (Fn key or the System page's slider) and note what
   each does to the colour. Then at level 3 run `set ff0000 50` and
   `set ff0000 0`, and repeat at level 1. Record whether they multiply, one
   overrides the other, or level 0 still turns it off.
4. Give it back: `... --lamp-array hidraw1 give-back`. The firmware's own
   lighting should return.
5. Set a colour, suspend, resume: does it stay? Reboot with nothing stored:
   is the firmware's lighting what it shows?

Each answer either confirms the follow-up in AGENTS.md or changes the
code: a level count read wrong, a kind the daemon refuses, a colour lost on
resume (that would need a resume hook).

Screenshots of the page states, taken in a headless sway sandbox against
`hardware_standin` (the real descriptor, a fake device behind it), are in
`/home/tim/Developer/better-os-evidence/136/`: the LampArray's own lighting,
steady colour (white, orange), `kbd_rgb_mode` not set, breathing and colour
cycle, one colour, no system service, and not allowed, in zh-TW and English,
light and dark. They show the stand-in's writes landing
(`0 1 0 160 255 0` in the fixture `kbd_rgb_mode`), not the real keyboard.
