# Touchpad input pass-through: security review

The review ADR 0008 and ADR 0012 require before any Better OS process reads a
touchpad. It covers the scroll pass-through that ticket 125 builds
(ADR 0030): `better-touchpad-scrolld`, the `touchpad-scroll` crate, its unit,
its udev rule, and its bus policy. Every claim below names the test, check, or
kernel rule that holds it; a claim with none is listed under "Not proven".

## What it does

GNOME 46 has no scroll-speed setting, and a GNOME Shell extension never sees a
client's scroll events (ADR 0029's experiment, ticket 88). The factor is
therefore applied below the compositor. For each touchpad, one instance of
`better-touchpad-scroll@eventN.service`:

1. opens `/dev/input/eventN` read-only and `/dev/uinput` write-only, as root;
2. judges the touchpad by its capability bits, and exits if it is not one;
3. drops to the `_better-touchpad-scroll` account;
4. while a factor other than 1 is set, grabs the touchpad exclusively
   (`EVIOCGRAB`), creates a uinput clone with the same capabilities, and
   forwards every event to the clone with two-finger and wheel motion scaled.
   libinput sees the clone as the touchpad; the original delivers nothing.

At factor 1 on both axes it holds nothing: no grab, no clone, no reads.

## The risk being reviewed

A process that can read one input node can, with the same permissions, read
any other: a keyboard's node is the same kind of file in the same directory
with the same default owner. A process that can write `/dev/uinput` can create
a keyboard and type into the session, including into a terminal. So the review
has to establish three things, in this order of importance:

1. it can never read or forward a key event, from a keyboard or from anything;
2. it can never leave the touchpad unusable;
3. if it is taken over, the attacker gains as little as possible.

## Threat model

| Who | Wants | Where they stand |
| --- | --- | --- |
| A local program running as the person | to read keystrokes, or to inject them | can call the service's bus methods; cannot write its files or ptrace it (different uid) |
| Another local account (a second user, a service) | to change the first person's touchpad, or the above | same as above, plus not in the active session |
| A malicious USB device | to be grabbed and read, or to feed a parser a hostile stream | can present any capability bits and any event stream |
| A bug in the pass-through itself | — | the realistic case: a panic, a hang, a lost event |

Out of scope: root (it owns the machine already), and the kernel's own evdev
and uinput drivers.

## Who holds which privilege

| Process | Runs as | Can open | Bus | Network |
| --- | --- | --- | --- | --- |
| `better-touchpad-scrolld eventN`, first ~1 ms | root, `CapabilityBoundingSet=CAP_SETUID CAP_SETGID` | `/dev/input/eventN` (read) and `/dev/uinput` (write), nothing else (device cgroup) | none yet | none (`PrivateNetwork=yes`, `AF_UNIX` only) |
| the same process afterwards | `_better-touchpad-scroll`, no supplementary groups, no capabilities, `no_new_privs` | nothing new: it uses the two descriptors it has | owns `org.betteros.TouchpadScroll1.eventN` on the system bus | none |
| `better-touchpad` (the window) | the person | no device | calls `SetFactors` and `Describe` | none |
| `udevd` | root | — | — | — |

Nothing in the window or a command line reads a device or mutates the system:
the window asks the service, and the service decides (AGENTS.md's privileged-
code rule). `crates/touchpad-platform/src/scroll.rs` holds no device code, and
the only `ioctl` in the workspace for these nodes is
`crates/touchpad-scroll/src/linux.rs`.

## Exactly which nodes it may open

Four layers, each enough on its own to stop a keyboard being opened by this
service, and none trusting the one above it:

1. **udev starts it only for touchpads.**
   `70-better-touchpad-scroll.rules` starts an instance only for an `event*`
   node with `ID_INPUT_TOUCHPAD=1`, and never for one with
   `ID_INPUT_KEYBOARD=1` or `ID_INPUT_KEY=1`, and never for a clone (`phys`
   `better-touchpad-scroll/*`). The postinst starts instances for existing
   touchpads from `better-touchpad-scrolld --eligible`, which judges sysfs's
   capability files by the same rule as layer 3, without opening any node.
   *Held by:* `verify-deb.sh` (the rule ships and names the template),
   `sysfs::tests::only_the_touchpad_is_eligible`,
   `maintainer-scripts.test.sh` ("starts one pass-through per touchpad",
   and a line from `--eligible` that is not an event name is never started).
2. **The kernel's device cgroup allows two nodes.** The unit sets
   `DevicePolicy=closed`, `DeviceAllow=/dev/input/%I r`, and
   `DeviceAllow=/dev/uinput w`. An `open` of any other device node fails with
   `EPERM` whatever the uid or the file's mode, so even root inside this unit
   cannot open the keyboard. The instance name is checked to be `event` and
   digits before it is joined to `/dev/input` (`sysfs::is_event_name`).
   *Held by:* `verify-deb.sh` (exactly two `DeviceAllow=` lines, these two),
   `sysfs::tests::event_names_cannot_leave_dev_input`.
3. **The service judges the opened node by what it can report.**
   `caps::classify` refuses, before anything is grabbed:
   - any key code outside the button block `BTN_MISC..KEY_OK` (0x100–0x15f),
     which is every keyboard key, every media and remote key, and every
     joystick trigger; one such bit refuses the whole device, so a combined
     keyboard-and-touchpad receiver is refused;
   - any event type other than `SYN`, `KEY`, `REL`, `ABS`, `MSC`, so anything
     with LEDs, autorepeat, switches, sound, or force feedback;
   - a touchscreen (`INPUT_PROP_DIRECT`), a tablet (pen or stylus), anything
     without a finger tool or an x and y axis, an axis with no range;
   - a clone of its own.

   The device's name is never consulted.
   *Held by:* the `caps::tests` module, among them
   `a_keyboard_is_refused_by_its_first_key`,
   `a_touchpad_that_also_reports_one_key_is_refused` (`KEY_A`, `0xff`,
   `KEY_OK`, `KEY_MAX`, `BTN_TRIGGER_HAPPY`),
   `a_keyboard_dressed_as_a_touchpad_is_still_a_keyboard`,
   `leds_autorepeat_switches_and_force_feedback_refuse_a_device`,
   `touchscreens_and_tablets_are_not_touchpads`,
   `the_pass_through_never_holds_its_own_clone`.
4. **The path must be the device it claims to be.** `linux::EvdevSource::open`
   opens with `O_NOFOLLOW` and refuses anything that is not a character device
   with the input major (13); `UinputFactory::open` refuses anything that is
   not 10:223. *Held by:*
   `linux::tests::a_path_that_is_not_an_input_node_is_refused_before_any_ioctl`
   (a regular file and `/dev/null` are both refused).

## It never reads or forwards a key event

- A device that can report a key is never held (layer 3), so no key event is
  ever read from a held device: the kernel does not emit codes a device did not
  declare.
- The clone is built only through `caps::clone_descriptor`, which classifies
  again and copies only the accepted device's capabilities. A uinput device
  drops any event whose code it did not declare, so even a bug that wrote a
  keyboard key to the clone would reach nobody.
  *Held by:* `caps::tests::a_clone_declares_no_key_and_cannot_be_made_from_a_keyboard`.
- Nothing is logged per event. The journal gets one line per mode change and
  one cost summary a minute; no coordinate, button, or timing is written
  anywhere.
- The process has no network (`PrivateNetwork=yes`,
  `RestrictAddressFamilies=AF_UNIX`), and the bus interface returns only the
  touchpad's name and the two factors.

## It never leaves the touchpad unusable

| Way out | What releases the touchpad | Held by |
| --- | --- | --- |
| factor set back to 1 | the loop waits until nothing is touching, lifts anything still down on the clone, ungrabs, then destroys the clone | `passthrough::tests::returning_to_neutral_releases_after_the_fingers_lift` |
| normal stop (`SIGTERM`, `SIGINT`, removal's `systemctl stop`) | the handler sets a flag the loop checks every 250 ms; `Passthrough`'s `Drop` lifts, ungrabs, and destroys | `passthrough::tests::dropping_the_pass_through_releases_the_grab_and_destroys_the_clone`; `TimeoutStopSec=3` |
| an error return, any `?` | the same `Drop` | the same test |
| a panic | `Drop` runs while unwinding | `passthrough::tests::a_panic_while_holding_releases_the_grab` |
| `SIGKILL`, an abort, an out-of-memory kill | the kernel: an `EVIOCGRAB` belongs to the open file and ends when it is closed, and a uinput device is destroyed when its descriptor is closed; a dead process's descriptors are all closed | kernel behaviour (`evdev_release`, `uinput_release`), not a test of ours |
| a hang (a deadlock, a stuck loop, `SIGSTOP`) | `WatchdogSec=5`: the loop sends `WATCHDOG=1` from the thread that forwards events; no heartbeat for 5 s and systemd kills the process, and the row above applies | `notify::tests::a_notification_reaches_the_socket_systemd_names`; `verify-deb.sh` requires `WatchdogSec=` |
| a crash loop | `StartLimitBurst=5` in 60 s, then systemd stops trying; a dead instance holds nothing | unit file |
| lost events (`SYN_DROPPED`) | everything is lifted on the clone, and nothing is forwarded until the pad is untouched, then forwarding resumes from a clean state | `passthrough::tests::lost_events_lift_everything_and_wait_for_an_untouched_pad` |
| the touchpad unplugged | the next ioctl or read answers `ENODEV` and the instance exits | `linux.rs`, read and `touching` |
| a hold taken mid-gesture | never: the hold is only taken when the kernel reports nothing touching, and the queue is drained first | `passthrough::tests::a_factor_takes_the_hold_only_while_the_pad_is_untouched` |
| a bad stored factor | `Factors::load` returns neutral, which holds nothing | `factors::tests::a_missing_or_bad_file_holds_nothing` |
| uninstall | prerm stops every instance; postrm deletes the account and the state | `maintainer-scripts.test.sh` |

The person's own escape: set the factors back to 1 in Better Touchpad (or
restore the captured settings, which were 1), remove the component (prerm
stops every instance), or `sudo systemctl stop 'better-touchpad-scroll@*'`.

## Who may change the factor

Anyone may call the interface (the bus policy allows sending to it), and the
service decides. `SetFactors` is accepted only when the caller's user has a
display session that logind reports active and not remote, the rule polkit's
`allow_active` expresses (`session_rule::decide`). root is allowed. Values are
refused unless both are valid `ScrollFactor`s (0.2 to 5.0, finite), the same
bound the window and the configuration file use.

polkit is not used. The service is unprivileged by the time a call arrives,
and polkit 0.105 (Ubuntu 22.04) answers an unprivileged mechanism only about
its own processes. Asking logind directly gives the same answer for this one
decision. *Held by:* `session_rule::tests::only_the_person_at_the_machine_may_change_the_factor`,
`factors::tests::the_bounds_are_the_settings_windows_bounds`.

The bus policy lets only `_better-touchpad-scroll` own a name under
`org.betteros.TouchpadScroll1`, so nothing else can pose as a pass-through to
the window.

## What an attacker gains

| Compromise | Gains | Does not gain |
| --- | --- | --- |
| Calls the bus methods as another local user | nothing: refused | — |
| Calls them as the person at the machine | scroll faster or slower, within 0.2–5 | anything else; the interface has no other method and takes no string |
| Takes over the process (a parser bug in the event loop) | the touchpad's events (finger positions, clicks) for as long as it runs, and a uinput descriptor that can create *a new device* with any capabilities, including a keyboard, and type into the session | any other input node (device cgroup), root (dropped, `no_new_privs`, bounding set), the network, the person's files (`ProtectHome`, `ProtectSystem=strict`, another uid), other processes (different uid) |
| Presents a hostile USB device | a pass-through for it only if it looks exactly like a touchpad and reports no key; its stream is parsed as fixed-size `input_event` records by a loop with no allocation driven by the stream | — |

The second-to-last row is the residual risk the owner accepted on 2026-11-15:
a process holding `/dev/uinput` can synthesise a keyboard. It is bounded by
the process having nothing that parses untrusted variable-length input (events
are 24-byte records the kernel produced), by two methods taking two doubles as
its only other input, and by the systemd sandbox. Narrowing it further would
need the clone to be created by a process that then gives up `/dev/uinput`,
which uinput does not support: the descriptor that created the device is the
one that writes to it.

## Not proven

- Nothing here has run against a real touchpad, a real `/dev/uinput`, or a real
  system bus with logind. Tests use the fake device seam
  (`passthrough::fake`) and a private `dbus-daemon`. The one-command procedure
  for the owner's machine is in
  [touchpad-scroll-performance.md](/engineering/performance/touchpad-scroll-performance.md#owner-run).
- That systemd expands `%I` inside `DeviceAllow=` is read from systemd's
  documentation of unit specifiers, not observed. The owner run checks it
  (`systemctl show -p DeviceAllow`); if it were not expanded, the unit would
  fail to open the touchpad (the path would be wrong), which fails closed.
- libinput's handling of the clone (palm detection, disable-while-typing,
  tap) is reasoned from libinput's documented use of the device's
  capabilities, bus type, vendor, product, name, and
  `ID_INPUT_TOUCHPAD_INTEGRATION`, all of which the clone carries over. Not
  observed. A touchpad libinput groups with a trackpoint loses that grouping
  on the clone.
