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
   `90-better-touchpad-scroll.rules` starts an instance only for an `event*`
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
- After the privilege drop the service installs its own seccomp filter
  (`seccomp.rs`) that checks every `ioctl`'s arguments: `UI_SET_KEYBIT` only
  for codes `0x100..0x160`, `UI_SET_EVBIT` only for `SYN`, `KEY`, `REL`,
  `ABS`, `MSC`, and never `UI_SET_SWBIT`, `UI_SET_LEDBIT`, `UI_SET_FFBIT`, or
  `UI_SET_SNDBIT` (`EPERM`). A system call from another architecture or the
  x32 ABI kills the process. So not even code running inside the process can
  declare a keyboard key on any uinput device it creates.
  *Held by:* `seccomp::tests::the_filter_refuses_every_keyboard_capability_in_a_child_process`
  (a forked child installs the filter, then `KEY_A`, `0xff`, `KEY_OK`, a key
  with the high word set, `EV_LED`, `EV_REP`, and the four forbidden requests
  fail with `EPERM` while `BTN_LEFT`, `BTN_TOOL_DOUBLETAP`, and `EV_ABS` reach
  the kernel), `seccomp::tests::every_jump_lands_inside_the_program`.
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
| the touchpad unplugged | the unit is `BindsTo=` its device and stops; if the node answers `ENODEV` first the instance exits with a failure. Either way a node that returns under the same name (a replug, a resume) gets a fresh instance | `verify-deb.sh` requires `BindsTo=` and `After=`; `main.rs` |
| a hold taken mid-gesture | never: the queue is drained, the kernel must report nothing down, and the grab is taken; then the queue is read again and the kernel asked again, and any event that arrived between the check and the grab, or anything down, gives the pad back at once (ungrab, clone destroyed) and tries again on a later turn. Without that, a press libinput saw from the original would be released only on the clone, and the original would keep a button down | `passthrough::tests::a_touch_between_the_check_and_the_grab_gives_the_pad_back`, `passthrough::tests::an_event_that_arrived_before_the_grab_gives_the_pad_back`, `passthrough::tests::a_factor_takes_the_hold_only_while_the_pad_is_untouched` |
| a resting finger after a scroll | the offset a scroll added is brought back to the finger's real position within a few frames once two-finger mode ends, even if the finger sends no motion, so a click lands in the clickpad software-button zone under the finger | `transform::tests::a_click_after_a_scroll_lands_where_the_finger_really_is` |
| a bad stored factor | `Factors::load` returns neutral, which holds nothing | `factors::tests::a_missing_or_bad_file_holds_nothing` |
| uninstall | prerm stops every instance; postrm stops them again (one udev started in between), deletes the state, and deletes the account, saying so with the command to finish when `userdel` fails | `maintainer-scripts.test.sh` |

The person's own escape: set the factors back to 1 in Better Touchpad (or
restore the captured settings, which were 1), remove the component (prerm
stops every instance), or `sudo systemctl stop 'better-touchpad-scroll@*'`.

## Who may change the factor

Anyone may send to a pass-through's name (the bus policy allows sending to
names under `org.betteros.TouchpadScroll1` and that interface only), and the
service decides, per calling **session**, not per user (`session_rule`):

- it asks the bus for the caller's process id and logind for that process's
  session (`GetSessionByPID`); the session must be local and the active one on
  its seat. The same person's ssh session, or a session switched away from,
  is refused even though the uid matches;
- a process in no session at all (a program started from the application
  grid runs under the user's service manager) is judged by the user's display
  session instead, which must be local and active: what polkit does for the
  same case;
- root is allowed.

Values are refused unless both are valid `ScrollFactor`s (0.2 to 5.0,
finite), the same bound the window and the configuration file use. Setting
the factors already in force writes nothing and logs nothing; a refusal is
logged at most once every 10 seconds with a count of the ones swallowed, so a
caller retrying in a loop cannot fill the journal.

polkit is not used. The service is unprivileged by the time a call arrives,
and polkit 0.105 (Ubuntu 22.04) answers an unprivileged mechanism only about
its own processes. *Held by:* `session_rule::tests::the_calling_session_decides_not_the_user`,
`session_rule::tests::a_process_outside_every_session_falls_back_to_the_display_session`,
`session_rule::log_tests::a_burst_of_refusals_is_one_line_and_a_count`,
`factors::tests::setting_the_factors_in_force_writes_nothing`,
`factors::tests::the_bounds_are_the_settings_windows_bounds`.

The bus policy lets only `_better-touchpad-scroll` own a name under
`org.betteros.TouchpadScroll1`, so nothing else can pose as a pass-through to
the window. On Ubuntu 24.04 sends are narrowed with
`send_destination_prefix`; Ubuntu 22.04's dbus-daemon 1.12 cannot read that
attribute (it arrived in the 1.13 series, per dbus's NEWS), so the 22.04
package ships a policy narrowed by interface only. The postinst reloads the bus
configuration after creating the account, because dpkg installs the policy
before the account exists and the bus drops a rule naming an unknown user.
*Held by:* `verify-deb.sh` (per release), `maintainer-scripts.test.sh`.

## What an attacker gains

| Compromise | Gains | Does not gain |
| --- | --- | --- |
| Calls the bus methods as another local user | nothing: refused | — |
| Calls them as the person at the machine | scroll faster or slower, within 0.2–5 | anything else; the interface has no other method and takes no string |
| Takes over the process (a parser bug in the event loop) | the touchpad's events (finger positions, clicks) for as long as it runs, and a uinput descriptor that can create a new device with a touchpad's kind of capabilities: buttons, axes, wheels. It can move the pointer and click | a keyboard key on any device (the seccomp filter), any other input node (device cgroup), root (dropped, `no_new_privs`, bounding set), the network, the person's files (`ProtectHome`, `ProtectSystem=strict`, another uid), other processes (different uid) |
| Presents a hostile USB device | a pass-through for it only if it looks exactly like a touchpad and reports no key; its stream is parsed as fixed-size `input_event` records by a loop with no allocation driven by the stream | — |

The third row is the residual risk the owner accepted on 2026-11-15. An
earlier draft of this review said it could not be narrowed below "a process
holding `/dev/uinput` can synthesise a keyboard"; that was wrong. uinput
cannot give the descriptor up after creating a device, but the process can
take away its own ability to declare keys, and since ticket 125's review fixes
it does: what is left is a pointing device, which can move the pointer and
click but cannot type. It is further bounded by the process having nothing
that parses untrusted variable-length input (events are 24-byte records the
kernel produced), by two methods taking two doubles as its only other input,
and by the systemd sandbox.

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
  tap, trackpoint pairing) is reasoned from libinput's documented use of the
  device's capabilities, bus type, vendor, product, name,
  `ID_INPUT_TOUCHPAD_INTEGRATION`, and `LIBINPUT_DEVICE_GROUP`, all of which
  the clone carries over: the last two are read from the original's udev
  database entry before the drop, written into the clone's `phys`, and given
  back to the clone by the rule (numbered 90, after systemd's and libinput's
  own rules) through `--clone-properties`, which reads only that one sysfs
  file and accepts only the exact shape the service writes
  (`udev::tests`). Not observed on a desktop.
- That `GetSessionByPID` answers "no session" for a program started from the
  application grid, which is why the display-session fallback exists, is
  read from logind's design, not observed here.
- That a `udevadm trigger --action=change` makes systemd start the instance
  for a touchpad present at install is read from systemd's handling of
  `SYSTEMD_WANTS` on a device that newly gains the `systemd` tag; the owner
  run checks that an instance runs after install.
