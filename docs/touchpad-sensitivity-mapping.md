# Better Touchpad: what each control maps onto, and what it cannot promise

Better Touchpad presents its own scales and its own vocabulary. The GNOME
backend maps those onto `org.gnome.desktop.peripherals.touchpad` keys. This note
records the exact mapping, the places it is lossy, and the measurements taken
against it, so nothing about the translation has to be reverse-engineered from
`crates/touchpad-platform/src/gnome.rs`.

## The scales

| Better OS value | Range | Meaning at the ends | Neutral |
| --- | --- | --- | --- |
| Pointer sensitivity | `0.0 .. 1.0` | slowest / fastest the backend offers | `0.5` |
| Scroll factor | `0.2 .. 5.0` | a fifth of / five times the session's own scroll distance | `1.0` |

Both are Better OS scales, not backend numbers. A backend range is a backend
detail and would change under the user if the backend changed. The bounds are a
recorded starting point, not a settled curve — see
[ADR 0010](decisions/0010-touchpad-ranges-and-dconf-writes.md).

Values outside a range are **rejected, never clamped**. A slider cannot produce
one, because the slider's own bounds are the supported range; a configuration
file or a migration can, and it is refused with the bound named.

## The GNOME mapping

Ten of the thirteen controls have a GNOME key. All ten are `d`, `b`, or `s`
GVariant values under `/org/gnome/desktop/peripherals/touchpad/`, all apply
immediately, and all are verified by reading the key back.

| Control | GNOME key | Type | Conversion |
| --- | --- | --- | --- |
| Pointer sensitivity | `speed` | `d` | `gnome = better × 2 − 1` |
| Acceleration profile | `accel-profile` | `s` | `default` / `adaptive` / `flat`, one to one |
| Disable while typing | `disable-while-typing` | `b` | one to one |
| Natural scrolling | `natural-scroll` | `b` | one to one |
| Two-finger scrolling | `two-finger-scrolling-enabled` | `b` | one to one |
| Tap to click | `tap-to-click` | `b` | one to one |
| Tap and drag | `tap-and-drag` | `b` | one to one |
| Drag lock | `tap-and-drag-lock` | `b` | one to one |
| Click method | `click-method` | `s` | `default` / `areas` / `fingers` / `none`, one to one |
| Middle-click emulation | `middle-click-emulation` | `b` | one to one |

### What the pointer mapping cannot promise

- **It is linear on GNOME's number, not on the pointer.** GNOME hands `speed`
  to libinput, which applies its own non-linear acceleration curve on top.
  Better OS 25% is not "a quarter of the pointer speed"; it is the quarter point
  of the range GNOME offers. Nothing here can promise otherwise without
  measuring libinput's curve, which is device-dependent.
- **It is not exactly invertible in binary floating point.** Writing `0.35` and
  reading it back can differ in the last bit. Comparisons therefore agree to
  `1e-9` (`touchpad_platform::VALUE_TOLERANCE`); an exact comparison would
  report a partial success for a write that was exact.
- **A value outside `-1.0 ..= 1.0` in the database is refused, not clamped.**
  Somebody else wrote it, and rounding it into range would present a value
  nobody chose as the current setting.

### The three controls GNOME has no key for

GNOME 46 — the release Zorin 18 ships — has no key for these, so they are shown
as unavailable with the reason attached and are never written:

| Control | Reason key | What the screen says |
| --- | --- | --- |
| Vertical scroll factor | `gnome.no_scroll_factor_key` | GNOME's touchpad settings have no scroll-factor key, so scrolling distance follows the pointer speed |
| Horizontal scroll factor | `gnome.no_scroll_factor_key` | as above |
| Smooth scrolling | `gnome.no_smooth_scroll_key` | decided by the compositor and each application |

This is the capability rule working, not a gap in the model: `touchpad-core`
carries both scroll factors as independent, linkable values, and the mock
backend applies and verifies both, so the behaviour is proven and ready for a
backend that has the key. Adding it when GNOME grows one is a row in `MAPPED`.

The scroll factors are also the reason the model keeps a requested value and an
effective value apart everywhere: on this backend the two genuinely differ for
those controls, and showing only one of them would be a lie either way.

### Hardware can override the backend

A control is offered only when the backend **and** the selected pad can carry
it. `DeviceCapabilities::limits` reports, from `/proc/bus/input/devices`:

- a pad reporting fewer than two contacts cannot two-finger scroll;
- a pad with a real middle button has nothing to emulate;
- a pad with separate hardware buttons has only one click method.

## Reading and writing

Reading is a GVDB parse of the user's own `~/.config/dconf/user`, through the
same reader ticket 27 built for Better Defaults. A key the database does not
hold is reported as **the session default** — a definite, restorable state —
rather than as unknown. Restoring such a key removes it again instead of writing
a value nobody chose.

Writing goes through `ca.desrt.dconf.Writer.Change` on the session bus. Nothing
edits the database file. See
[ADR 0010](decisions/0010-touchpad-ranges-and-dconf-writes.md) for why, and for
the change-set encoding.

## Measurements

Taken on the development host (Zorin OS 18.1, GNOME Shell 46, Wayland),
`--release` for the read figures and `debug` for the window, which is why the
startup figure is the pessimistic one.

| What | Figure | How |
| --- | --- | --- |
| Read every setting back | 6.8 µs | `cargo test -p touchpad-platform --test read_latency --release -- --nocapture` |
| Read one setting | 5.5 µs | same |
| Stage one slider move | 1.2 µs | `cargo bench -p touchpad-core` |
| Build an apply plan | 1.5 µs | same |
| Capture before the first write | 1.4 µs | same |
| Build a full restore plan | 1.9 µs | same |
| Migrate a version 1 file | 1.5 µs | same |
| Write a configuration and read it back | 4.7 µs | same |
| Apply one setting and verify it, against the real dconf service | 3.6 – 6.0 ms | `BETTER_TOUCHPAD_LIVE=1 cargo test -p touchpad-platform --test live_apply -- --ignored --nocapture` |
| Restore one setting and verify it, same service | 3.6 – 4.7 ms | same |
| Window ready from process start | 99 – 134 ms (debug build) | `BETTER_TOUCHPAD_TRACE_STARTUP=1 ZED_HEADLESS=1 better-touchpad --offline` |
| Of which reading the desktop | 0.33 – 0.54 ms | same |

Two consequences of these numbers are load-bearing:

- **No background task.** An apply and its verifying read cost about 4 ms, so
  the GUI does the work on the calling thread. A task and a channel would add
  more machinery than the work they moved.
- **No polling.** Nothing in Better Touchpad runs while idle. Values are read
  when the window opens, when the user asks, and after a change.

Not measured, and not claimed: pointer-event overhead and scroll-event latency.
Better Touchpad sits in no input path — it writes a setting and the compositor
does the rest — so there is no Better OS code between a finger and a pointer to
measure. Issue #3 lists both because a gesture adapter would have them; ticket
30 owns that, and the figures belong with it.
