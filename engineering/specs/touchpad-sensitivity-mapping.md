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
[ADR 0010](/engineering/decisions/0010-touchpad-ranges-and-dconf-writes.md).

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

GNOME 46 — the release Zorin 18 ships — has no key for the scroll factors or
for smooth scrolling.

**The two scroll factors are carried by the scroll pass-through** (ticket 125,
[ADR 0030](/engineering/decisions/0030-touchpad-scroll-passthrough.md)). One
system service per touchpad, `better-touchpad-scroll@eventN.service`, owns
`org.betteros.TouchpadScroll1.eventN` on the system bus. The GNOME backend's
`crate::scroll` client reads the factors in force from the first of them,
writes both factors to every one of them in one call (a change to one keeps the
other at what is in force), and verifies by reading back, like every other
control. Resetting a factor is setting it to 1, at which the service holds
nothing. The factors are offered exactly when a pass-through answers:

| State | Reason key | What the screen says |
| --- | --- | --- |
| a pass-through answers | — | the factors are live sliders, applied at the next touchpad frame |
| no touchpad has one | `gnome.scroll_passthrough_absent` | no touchpad has Better Touchpad's scroll service running, so scroll distance follows the pointer speed |
| the system bus fails | `gnome.scroll_passthrough_unreachable` | the bus error |
| smooth scrolling, always | `gnome.no_smooth_scroll_key` | whether scrolling animates is decided by each application |

What a factor does, exactly: while two fingers are on the pad, each finger's
movement is multiplied before libinput sees it, the vertical factor on the y
axis and the horizontal one on the x axis, so libinput's two-finger scroll
travels that much further or less far. Pointer motion and three- and
four-finger gestures are not scaled; a touchpad that reports a wheel has its
wheel (including the high-resolution one) scaled by the same factors. The
scaling rules are in `crates/touchpad-scroll/src/transform.rs`, and three
consequences of them belong on the record:

- A scaled finger stops at the pad's edge, so at factor 5 the scroll can end
  when the fingers are halfway across.
- A two-finger pinch is scaled too (both fingers move by the same factor), and
  GNOME 46 uses no two-finger pinch.
- The factors belong to the machine's touchpads, not to one person's session:
  every touchpad gets the same pair, and a person who logs in later scrolls with
  them until they change them. Only the person in the active local session may
  change them.

**Smooth scrolling stays unavailable.** A touchpad's scroll is already
continuous when it leaves libinput; whether a view glides on after the fingers
lift is each application's own decision (GTK and the browsers make it), and no
input-device setting reaches it. The pass-through changes how far a scroll
goes, not how an application draws it.

The model keeps a requested value and an effective value apart everywhere,
which is what lets a factor the service refused read as refused rather than as
applied.

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
[ADR 0010](/engineering/decisions/0010-touchpad-ranges-and-dconf-writes.md) for why, and for
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

Pointer-event overhead and scroll-event latency: with every factor at 1 Better
Touchpad sits in no input path. With a factor set, the scroll pass-through does,
and its added cost per frame is recorded in
[touchpad-scroll-performance.md](/engineering/performance/touchpad-scroll-performance.md):
about 60 ns to scale a frame and about 160 ns for the whole forwarding turn in
process, with the kernel's evdev-to-uinput hop owed to a run on real hardware.
