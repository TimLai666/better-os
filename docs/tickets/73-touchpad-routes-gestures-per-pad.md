# Ticket 73 — Each touchpad performs its own gesture profile

**Epic:** Better Touchpad (Issue #3) · **User Story:** a person with a laptop
touchpad and an external one gets each pad's own gestures · **Branch:**
`ticket-73` · **Blocked by:** none · **Status:** merged, not released

## What it delivers

Gesture profiles are per device, but `org.betteros.TouchpadAdapter1` reports no
device identity, so both pads perform the selected pad's profile. The owner
decided on 2026-09-27 to widen the adapter interface so each gesture event
names the device that produced it. That changes the GJS exception's bounds, so
ADR 0015 records the widening first, as `AGENTS.md` requires.

## Acceptance criteria

- [x] ADR 0015 states exactly what the extension may now report about a device
      (an identity the shell already exposes, nothing it computes or stores),
      and `crates/touchpad-session/tests/extension.rs` asserts the new bound.
      *`docs/decisions/0015-touchpad-adapter-reports-the-device.md`;
      `the_extension_reports_the_device_node_and_nothing_else_about_the_device`
      and `both_gesture_signals_have_the_shape_the_client_deserializes`.*
- [x] Every swipe and pinch event carries the device identity when the shell
      provides one, and an explicit "unknown device" when it does not.
      *Both signals end in `s device`, the empty string when unknown.
      `a_device_node_is_read_only_in_the_shape_the_kernel_gives_it` and
      `a_gesture_event_carries_the_device_that_made_it` in
      `crates/touchpad-session/src/gnome.rs`.*
- [x] `better-touchpad-gestured` recognizes each event against that device's
      profile, falls back to the global profile for an unknown or unprofiled
      device, and never mixes frames from two devices into one gesture.
      *`crates/touchpad-gesture-service/tests/devices.rs`.*
- [x] Tests drive the Rust fake of the adapter with two devices. The live
      extension is checked on a nested GNOME Shell if one can be started, and
      the ticket says which it was.
      *A headless GNOME Shell 46.0 in an isolated session; see below.*

## What was built

### What the extension reports

Each `SwipeGesture` and `PinchGesture` signal now has a sixth argument,
`device`. It is `event.get_source_device().get_device_node()`, for example
`/dev/input/event5`, copied onto the signal as the shell gives it, or the empty
string when the shell names no node. The extension reads no other fact about a
device, computes nothing from the node, and keeps nothing about any device. The
capability report says `protocol_version` 2.

ADR 0015 records why the node and not the name or the vendor and product ids.
The node is the one value the shell exposes that names exactly one device and
leads to the identity a profile is filed under. Clutter has no bus, version, or
`Uniq`, so a name and two ids cannot rebuild either identity shape, and two pads
of the same model share all three.

`crates/touchpad-session/tests/extension.rs` asserts the bound. Both signals
must be `u u d d t s`. The extension must call `get_source_device()` and
`get_device_node()` and none of the other device accessors. It must not name a
checksum, hash, string operation, `Map`, or `Set`. And the fields it assigns
must be exactly the five it had before this ticket, so a device cache would
fail the test.

### From the node to a profile

`touchpad_session::InputDeviceNode` is the node read into a number at the bus
boundary. Anything that is not exactly `/dev/input/event` and a number in the
kernel's spelling is unknown. When a gesture begins, `better-touchpad-gestured`
reads `/sys/class/input/event<N>/device` for the `uniq`, bus, vendor, product,
version, and name, and builds the identity with `touchpad_core::device_identity`.
That is the same function `touchpad-platform` now uses for the identities it
reads from `/proc/bus/input/devices`, so the two cannot spell one pad two ways.
The node and the identity are not kept after the gesture's profile is chosen.

The pipeline holds every stored profile and one recognizer for each profile that
is switched on, so each profile has its own cooldown. A device with its own
profile uses it. An unknown device, a node with nothing readable behind it, and
a device with no profile of its own use the global profile. A device whose own
profile is off recognizes nothing, rather than borrowing the global one.

A gesture belongs to the device that began it. An event from any other device
while it is under way cancels it on its own profile before that event is read.
A Begin on the same node that now resolves to another profile does the same.

The service also reads the five-argument signals of a protocol 1 extension as
coming from an unknown device. GNOME Shell keeps running the extension it loaded
until the next login, so after an upgrade the old shape still arrives for one
session, and those gestures stay on the global profile instead of being dropped.

Three things changed shape because a pipeline now holds several profiles:

- Verification binds every profile and writes all of them back. The state it
  returns is the worst any profile reached.
- Suppressing the desktop's own gestures is wanted when any switched-on profile
  takes one. GNOME 46's swipe trackers serve every pad, so a gesture one pad's
  profile takes from the desktop is taken on every pad.
- Auto-disable turns every profile off, because every pad shares the adapter
  that failed. Saving replaces the global profile and the device profiles the
  stored document still has, and leaves the window's selected pad alone.

`GestureStore::load_active_config` is gone. Its only caller was the service, and
its premise — one profile for every pad — is what this ticket removes.

### Tests

- `crates/touchpad-gesture-service/tests/devices.rs` drives the shipped pipeline
  with a recorded event stream, the fake shell bridge, and a temporary `/sys`
  holding three pads: a laptop pad filed by bus, vendor, product, version, and
  name, a Bluetooth pad filed by `Uniq`, and a pad with no profile. Nine tests:
  each pad performs its own profile; unknown, unprofiled, and unreadable devices
  fall back to the global profile; a pad whose own profile is off does nothing;
  another pad beginning a gesture cancels the one under way on its own profile;
  one stray event from another pad ends the gesture under way; half a swipe on
  each of two pads following one profile is not a swipe; a pipeline with no
  device resolver performs the global profile everywhere; the sysfs reader gives
  the identities `touchpad-platform` would; and a failing adapter turns every
  profile off in the stored file.
- `crates/touchpad-gesture-service/tests/pipeline.rs`, on a private
  `dbus-daemon`: the fake extension now emits the six-argument signals. Two new
  tests show that the device named on the wire picks that pad's own profile
  across a real bus, and that a protocol 1 five-argument signal still produces
  a gesture on the global profile.
- `touchpad-core` has a test for both identity shapes, and `touchpad-platform`'s
  existing identity tests now run through the shared function.

Before the change, the two extension tests failed: the signals had five
arguments, and the extension did not read `get_source_device()`. The new unit
tests and both integration test files failed to compile for want of
`InputDeviceNode`, `device_identity`, `GesturePipeline::with_profiles`, and the
device resolvers. With the device check in the pipeline deliberately removed,
three of the `devices.rs` tests fail, which is how the mixing rule is known to
be tested rather than passing by accident.

### The live shell

Run on this host's GNOME Shell 46.0 (Mutter 46.2) as `gnome-shell --headless
--wayland --no-x11 --virtual-monitor 1024x768` under `dbus-run-session`. `HOME`,
`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, and
`XDG_RUNTIME_DIR` all pointed at temporary directories. The extension was copied
into the temporary data directory and enabled with `gsettings` in that session's
own dconf database. `XDG_RUNTIME_DIR` has to be a short path, because the
Wayland socket path must fit in 108 bytes.

`gnome-extensions info` reported it `Enabled: Yes`, `State: ACTIVE`. The session
bus showed `org.betteros.TouchpadAdapter1` owned. Introspection showed both
signals ending in `s device` and the five methods unchanged. `Capabilities`
returned `protocol_version` 2 and `built_in_trackers` 2.
`SuppressBuiltInGestures(true)` flipped `built_in_gestures_suppressed` to true,
and `false` put it back. The shell log had no JavaScript error. Before and after,
the host's `~/.config/dconf/user` had the same modification time, size, and
SHA-256. The host's `enabled-extensions` value was unchanged, and
`~/.local/share/gnome-shell/extensions` still did not exist.

No gesture event was seen. A shell with no touchpad produces none, so the node
Clutter names for a real event's source device has still not been observed
against a hand.

### Not done, and why

- `AGENTS.md` still says nothing routes a gesture to the pad that produced it,
  and ADR 0010's open question about which pad a gesture came from is now
  answered by ADR 0015. Neither file was in this ticket's scope to edit.
- The capture beside `gestures.json` is still one file for the machine, as
  before.

### Gates

Run from this worktree with its own target directory and
`RUST_FONTCONFIG_DLOPEN=1`:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p <crate> --all-targets --all-features --locked -- -D warnings`
  for `touchpad-core`, `touchpad-platform`, `touchpad-session`,
  `touchpad-gestures`, and `touchpad-gesture-service`: clean on stable and on
  1.98.0.
- `cargo test -p <crate> --locked`: `touchpad-core` 66; `touchpad-platform` 61
  unit, 12 manifest, 2 latency, 2 ignored live; `touchpad-session` 31 unit,
  7 extension; `touchpad-gestures` 116; `touchpad-gesture-service` 9 devices,
  11 private-bus.
- `cargo check -p touchpad-gui --all-targets --locked`: builds against the
  changed crates.
- `Cargo.lock` did not change.
