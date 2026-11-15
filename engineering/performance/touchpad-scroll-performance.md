# Touchpad scroll pass-through: added latency

What `better-touchpad-scrolld` (ticket 125, ADR 0030) adds between a touchpad
and libinput while a scroll factor other than 1 is set. At factor 1 on both
axes it holds nothing and adds nothing: libinput reads the touchpad directly.

## Recorded

Host: AMD Ryzen AI 9 HX 370 (24 threads), Zorin OS 18.1, release build,
`cargo bench -p touchpad-scroll` (`crates/touchpad-scroll/benches/transform.rs`),
200 000 two-finger frames of ten events each, 2026-11-15.

| What | Mean | p50 | p99 | Max |
| --- | --- | --- | --- | --- |
| Scale one frame (`Scaler::frame`) | 57 ns | 60 ns | 80 ns | 35 µs |
| One whole forwarding turn: frame assembly, scaling, hand-off to the clone (fake device and clone) | 163 ns | 160 ns | 241 ns | 18 µs |

The maxima are single outliers (a scheduler preemption in a 200 000-sample
run), not a steady cost. A touchpad reports a frame every 7 to 12 ms, so the
in-process cost is under 0.003 % of the frame interval.

## Not in these figures

- **The kernel hop.** Reading the original node, writing the clone, and the
  kernel delivering the clone's event to libinput are two system calls and one
  evdev queue. They are not measured here, because this host's real
  `/dev/input` and `/dev/uinput` are never opened by a test. The service logs
  its own cost on real hardware once a minute while it holds a touchpad,
  measured from the moment a read returns to the moment the last frame in it
  was written to the clone:

  ```text
  better-touchpad-scroll@event7: /dev/input/event7: forwarded 5120 reads, added 4.1µs on average and 31µs at worst
  ```

  (shape only; no number has been recorded from real hardware yet).
- **Wake-up latency.** The loop blocks in `poll` and is woken by the kernel
  when an event arrives, so there is no polling interval in the path. The
  250 ms timeout only bounds how fast a stop request or a factor change to 1 is
  noticed.

## Owner run

The real-hardware numbers and the behaviour checks are owed to the owner's
machine. After installing the `better-touchpad` package built from the
branch:

```sh
bash packaging/tests/touchpad-scroll-hardware-check.sh
```

It checks that an instance runs for each touchpad and none for anything else,
that `DeviceAllow` expanded to the touchpad's own node, that the process runs
as `_better-touchpad-scroll`, that a factor of 3 scales two-finger scrolling
and leaves pointer motion, tapping, gestures, and disable-while-typing alone,
that a frozen service gives the touchpad back through the watchdog, puts the
factors back, and prints the logged per-read cost. Record that cost here.
