# ADR 0015: The touchpad adapter names the device behind each gesture

## Status

Accepted by the project owner on 2026-09-27. It widens a bound of the GJS
exception [ADR 0012](0012-touchpad-gesture-backend.md) granted, which is why it is
an ADR and not a commit.

## Context

Gesture profiles are per device since ticket 39 ([ADR
0010](0010-touchpad-ranges-and-dconf-writes.md), as amended). A profile is keyed
by the stable identity `touchpad-platform` derives: `uniq:<Uniq>` when the
kernel reports one, and `input:<bus>:<vendor>:<product>:<version>:<name>`
otherwise.

`org.betteros.TouchpadAdapter1` reports what a gesture did and nothing about
which pad did it. So `better-touchpad-gestured` recognizes every gesture against
one profile, and on a machine with a laptop pad and an external one both pads
perform the selected pad's gestures. The owner decided to widen the interface so
that each gesture event names the device that produced it.

The GJS exception says the extension may report what the compositor saw and may
hold no configuration and make no decision. The question this ADR answers is
exactly which fact about a device counts as "what the compositor saw", and what
the extension may do with it.

## Decision

**Each `SwipeGesture` and `PinchGesture` signal gains a sixth argument, `device`,
of type `s`. It is the device node of the event's source device, as GNOME Shell
already exposes it — `event.get_source_device().get_device_node()`, for example
`/dev/input/event5` — copied onto the signal unchanged. When the event has no
source device, or the device has no node, the argument is the empty string, and
the empty string means "unknown device".**

That is the whole of what the extension may report about a device. It may call
those two accessors and no other: not the device name, the vendor or product id,
the device type, or the seat. It may not compute, compare, hash, parse, or
transform the value, and it may not keep it: the node is read from the event it
belongs to, emitted with that event, and forgotten. The extension holds no
record of which devices exist or which one was seen last.

`PROTOCOL_VERSION` in the capability report becomes 2.

### Why the device node, and not the name or the ids

Clutter's `InputDevice` in GNOME 46 exposes four facts that could name a device:
the device node, the name, the vendor id, and the product id. The profile key
needs one that leads to the identity `touchpad-platform` already derives.

| Value from the shell | Names exactly one device now | Leads to the stored identity |
| --- | --- | --- |
| Device node (`/dev/input/event5`) | Yes. The kernel gives each input device its own node. | Yes. `/sys/class/input/event5/device` holds the same `uniq`, bus, vendor, product, version, and name the identity is made of. |
| Name, vendor id, product id | No. Two pads of the same model share all three. | No. Clutter exposes no bus, no version, and no `Uniq`, so neither identity shape can be rebuilt, and a `uniq:` identity — the usual one for a Bluetooth pad — cannot be matched at all. |

The node is not stable. It is whatever the kernel enumerated this boot, which is
exactly why ADR 0010 refused to file a configuration under it. That is also why
the node is never stored anywhere: `better-touchpad-gestured` turns the node into
the stable identity when a gesture begins, reading the kernel's own attributes in
Rust, and keeps neither. The identity rule itself stays in one place,
`touchpad_core::device_identity`, which both `touchpad-platform` and the service
call, so the two cannot build the same device's identity two different ways.

### How an unknown device is represented, and what it does

On the wire, unknown is the empty string. The Rust side goes further: anything
that is not exactly `/dev/input/event` followed by a number is also unknown. A
device that is known but has no profile of its own, a node whose kernel
attributes cannot be read, and an unknown device all recognize against the
global profile, which is what the profile document already promises for a pad
it has never seen.

Every unknown event is treated as coming from one device, because there is
nothing to tell two of them apart.

### Why this is still bridging a typed event

The exception allows the extension to report what the compositor saw. Which
device produced an event is part of the event: Clutter attaches it, and the
extension reads it from the event at the moment it reads the phase and the
deltas. Nothing is decided with it. The extension does not choose a profile,
filter a device, or treat one pad differently from another; that routing is
made in Rust, from the stored configuration, where every other gesture decision
already is.

The one new thing on the wire is a string, and the bound says no *method* may
take free text. The device node is a signal argument, flowing out of the shell
and not into it, so no caller can use it to ask the shell for anything. The Rust
side also parses it into a number before anything uses it, so a node that is not
a node can mean nothing but "unknown". The four action methods and their
arguments are unchanged.

### A gesture belongs to the device that began it

With two pads, two streams can interleave. `better-touchpad-gestured` gives each
gesture to the device that began it, and an event from any other device while
that gesture is under way cancels it before the new event is read. Frames from
two pads are never measured as one gesture. Two pads used at the same moment
therefore both lose the gesture, which is the safe outcome for a case nobody
does on purpose.

### Compatibility with a running protocol 1 extension

GNOME Shell on Wayland reloads an extension only when the session starts again.
After a package upgrade the shell can keep running the old extension while the
new service starts, so the service also accepts the five-argument signals of
protocol 1 and reads them as coming from an unknown device. That session keeps
its gestures on the global profile until the next login, rather than losing them.

## Consequences

- `crates/touchpad-session/tests/extension.rs` asserts the new bound against the
  file: the signal shapes end in `s`, the only device accessors the extension
  names are `get_source_device` and `get_device_node`, it names no other device
  fact and no hashing, and it keeps no field beyond the ones it had before this
  change. Any further widening fails a test.
- The desktop's own swipe trackers are still session-wide. GNOME 46 has one
  tracker for every pad, so if any pad's profile takes a gesture from the
  desktop, the desktop loses it on every pad. This ADR does not change that, and
  nothing in the shell would let it.
- The pointer, scrolling, and clicking settings stay session-global. ADR 0010's
  reason for that — one GNOME key for every pad — is untouched by this change.
- The service reads `/sys/class/input/event<N>/device` once per gesture begin.
  That is an unprivileged read of kernel attributes, the same kind
  `touchpad-platform` already makes.

## What was verified, and how

On GNOME Shell 46.0 (Mutter 46.2), run as `gnome-shell --headless --wayland
--no-x11 --virtual-monitor 1024x768` under `dbus-run-session`, with `HOME`,
`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, and
`XDG_RUNTIME_DIR` all pointed at temporary directories. The extension was enabled
in that session's own dconf database. The extension reported itself `ACTIVE`,
owned `org.betteros.TouchpadAdapter1`, and introspected with both signals
ending in `s device`. It reported `protocol_version` 2 and both swipe trackers,
and suppressed and restored them. The shell log had no JavaScript error. The
host's dconf database had the same modification time, size, and checksum
afterwards, and its `enabled-extensions` value was unchanged.

The device node itself was not observed on a live event. A shell with no
touchpad produces no gesture events, as ADR 0012 already records, so which
device Clutter names as an event's source device is taken from the API and not
seen against a hand. On the Rust side, the node's path to a profile is tested
with two devices against the fake adapter and under a temporary `/sys`.
