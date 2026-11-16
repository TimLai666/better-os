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
