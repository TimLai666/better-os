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

At factor 1 on both axes it holds nothing and reads nothing, but the clone
exists at every factor: it is created once, when the instance starts, so that
every later uinput setup call can be forbidden (below). At factor 1 it is a
silent twin, a second touchpad with the original's name that reports nothing,
and it is visible as such to libinput and in device listings (GNOME Settings,
`libinput list-devices`, `/proc/bus/input/devices`). Better Touchpad's own
device list leaves it out.

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
- The service filters its own system calls in two steps (`seccomp.rs`).
  Right after the privilege drop, every uinput capability call is checked
  against what a touchpad is: `UI_SET_KEYBIT` only for the mouse buttons and
  finger tools (`codes::TOUCHPAD_KEYS`: no key, no pen, no stylus),
  `UI_SET_RELBIT` only for the four wheel codes (no `REL_X`/`REL_Y`, so no
  relative mouse), `UI_SET_PROPBIT` only for pointer, buttonpad, semi-mt, and
  top-buttonpad (never `INPUT_PROP_DIRECT`, a touchscreen), `UI_SET_EVBIT`
  only for `SYN`, `KEY`, `REL`, `ABS`, `MSC`, and never `UI_SET_SWBIT`,
  `UI_SET_LEDBIT`, `UI_SET_FFBIT`, or `UI_SET_SNDBIT`. `caps::classify`
  accepts exactly the same three lists, so the service can build the clone
  of any device it holds. Then the clone is created, and a second filter
  forbids every uinput request (`'U'`) except `UI_DEV_DESTROY`: no
  `UI_DEV_SETUP`, `UI_ABS_SETUP`, `UI_SET_*`, or `UI_DEV_CREATE`, ever again.
  This happens before a single input event has been read.
  `UI_SET_ABSBIT` has no list of its own in the first step: whether a set of
  absolute axes is a touchpad or an absolute pointer (a touchscreen, a tablet,
  a virtual-machine tablet with `BTN_LEFT` and no finger tool) depends on the
  other capabilities beside it, which a filter on one call cannot see. The
  second step is what closes that: the only device the process can ever have
  declared is the clone `classify` judged, which has a finger tool and no
  `INPUT_PROP_DIRECT`, so libinput treats it as a touchpad, a relative pointer.
  A system call from another architecture or the x32 ABI kills the process.
  *Held by:* `seccomp::tests::step_one_allows_a_touchpad_and_nothing_that_types_or_points_absolutely`
  and `seccomp::tests::after_the_lock_no_uinput_device_can_be_declared_at_all`
  (both run the filter in a forked child and check which calls fail with
  `EPERM` and which reach the kernel), `seccomp::tests::the_filter_and_classify_accept_the_same_touchpad`,
  `seccomp::tests::every_jump_lands_inside_the_program`,
  `caps::tests::a_relative_mouse_or_a_pointing_stick_beside_a_finger_tool_is_refused`.
- Nothing is logged per event. The journal gets one line per mode change and
  one cost summary a minute; no coordinate, button, or timing is written
  anywhere.
- The process has no network (`PrivateNetwork=yes`,
  `RestrictAddressFamilies=AF_UNIX`), and the bus interface returns only the
  touchpad's name and the two factors.

## It never leaves the touchpad unusable

| Way out | What releases the touchpad | Held by |
| --- | --- | --- |
| factor set back to 1 | the loop waits until nothing is touching, lifts anything still down on the clone, and ungrabs; the clone stays, a silent twin | `passthrough::tests::returning_to_neutral_releases_after_the_fingers_lift` |
| normal stop (`SIGTERM`, `SIGINT`, removal's `systemctl stop`) | the handler sets a flag the loop checks every 250 ms; `Passthrough`'s `Drop` lifts, ungrabs, and destroys | `passthrough::tests::dropping_the_pass_through_releases_the_grab_and_destroys_the_clone`; `TimeoutStopSec=3` |
| an error return, any `?` | the same `Drop` | the same test |
| a panic | `Drop` runs while unwinding | `passthrough::tests::a_panic_while_holding_releases_the_grab` |
| `SIGKILL`, an abort, an out-of-memory kill | the kernel: an `EVIOCGRAB` belongs to the open file and ends when it is closed, and a uinput device is destroyed when its descriptor is closed; a dead process's descriptors are all closed | kernel behaviour (`evdev_release`, `uinput_release`), not a test of ours |
| a hang (a deadlock, a stuck loop, `SIGSTOP`) | `WatchdogSec=5`: the loop sends `WATCHDOG=1` from the thread that forwards events; no heartbeat for 5 s and systemd kills the process, and the row above applies | `notify::tests::a_notification_reaches_the_socket_systemd_names`; `verify-deb.sh` requires `WatchdogSec=` |
| a crash loop | `StartLimitBurst=5` in 60 s, then systemd stops trying; a dead instance holds nothing | unit file |
| lost events (`SYN_DROPPED`) | everything is lifted on the clone, and nothing is forwarded until the pad is untouched, then forwarding resumes from a clean state | `passthrough::tests::lost_events_lift_everything_and_wait_for_an_untouched_pad` |
| the touchpad unplugged | the unit is `BindsTo=` its device and stops; if the node answers `ENODEV` first the instance exits with status 69, which `RestartPreventExitStatus=69` keeps systemd from restarting, so a pad that drops off and returns never runs the unit into its start limit. Either way udev and `BindsTo=` give a node that returns under the same name (a replug, a resume) a fresh instance | `verify-deb.sh` requires `BindsTo=`, `After=`, and `RestartPreventExitStatus=69`; `ipc::EXIT_DEVICE_GONE` |
| a hold taken mid-gesture | never: the queue is drained, the kernel must report nothing down, and the grab is taken; then the queue is read again and the kernel asked again, and any event that arrived between the check and the grab, or anything down, gives the pad back at once (ungrab) and tries again on a later turn. Without that, a press libinput saw from the original would be released only on the clone, and the original would keep a button down | `passthrough::tests::a_touch_between_the_check_and_the_grab_gives_the_pad_back`, `passthrough::tests::an_event_that_arrived_before_the_grab_gives_the_pad_back`, `passthrough::tests::a_factor_takes_the_hold_only_while_the_pad_is_untouched` |
| a resting finger after a scroll | the offset a scroll added is dropped in the very frame where the finger count leaves two or a button goes down, for every contact, with a position event for each that sent none, so a click lands in the clickpad software-button zone under the finger | `transform::tests::lifting_one_finger_and_clicking_without_moving_clicks_where_the_finger_is`, `transform::tests::a_click_with_two_fingers_still_down_lands_where_they_are` |
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
  session instead, which must be local and active, and is refused while that
  user has any remote session open;
- root is allowed.

**This is not a boundary between processes of the same uid**, and it is
recorded as such. A process in no session can be the window, but it can as
well be the same person's cron job, a `systemd-run --user` unit, or something
an ssh login left running under the user's service manager; logind cannot
tell them apart. The remote-session refusal closes the ssh case while that
login is open, and nothing closes the rest. That is accepted: a process
running as the person can already change everything the person's desktop
can, and a scroll factor between 0.2 and 5 is the least of it. The rule keeps
other accounts and remote logins out.

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
the window, and no rule allows a send without a destination. On Ubuntu 24.04
sends are allowed to names under that prefix with `send_destination_prefix`.
Ubuntu 22.04's dbus-daemon 1.12 cannot read that attribute (it arrived in the
1.13 series, per dbus's NEWS), so the 22.04 package's policy names every
instance's own name, `event0` to `event511`, one `send_destination` rule each,
written by `packaging/touchpad/generate-bus-policy-1.12.sh`; a touchpad
numbered past `event511` on 22.04 is unreachable from the window, which shows
the pass-through as absent. The postinst reloads the bus configuration after
creating the account, because dpkg installs the policy before the account
exists and the bus drops a rule naming an unknown user.
*Held by:* `verify-deb.sh` (per release: the prefix on 24.04; on 22.04 512
named destinations and no send rule without one), the generator's `--check`
in `maintainer-scripts.test.sh`, `session_rule::tests`.

## Starting at install, with no reboot

The instances are `BindsTo=` their device, and systemd only has a device unit
for a node udev tagged `systemd`, which happens when udev processes the node
with this package's rule. For touchpads already present at install, the
postinst asks systemd to run `better-touchpad-scroll-coldplug.service`, a
one-shot unit whose only command is `udevadm trigger --action=change
--subsystem-match=input --property-match=ID_INPUT_TOUCHPAD=1`. It runs in
PID 1's context, not the maintainer script's, so it works when Better Manager
installs the package and the script runs inside the manager daemon's sandbox
(where `/sys` is read-only and a trigger from the script itself fails). The
postinst does not depend on its own output reaching anyone: when the unit
cannot run, it writes `/run/better-os/touchpad-scroll-reconnect`, and Better
Touchpad's Scrolling page shows "restart the computer or disconnect and
reconnect the touchpad" instead of the plain absent reason. The record is
under `/run`, so a restart clears it, and a later successful install or the
removal deletes it. *Held by:* `maintainer-scripts.test.sh`,
`gnome::tests::an_install_that_could_not_start_the_pass_through_says_restart_or_reconnect`,
`verify-deb.sh` (the unit's one command).

## Gestures on a held touchpad

While a touchpad is held, GNOME Shell sees its gestures from the clone's node.
The gesture service (ADR 0015's routing, `SysfsDeviceIdentities`) resolves a
clone's node, recognised by its `phys`, to the present non-clone device with
the same bus, vendor, product, version, and name, and uses that device's
identity, so the pad's own gesture profile applies. Two such devices with
different identities cannot be told apart, and the gesture uses the global
profile. *Held by:* `touchpad-gesture-service/tests/devices.rs`,
`a_gesture_on_a_held_pad_uses_that_pads_profile_not_the_global_one` and
`a_clone_that_matches_two_different_pads_goes_to_the_global_profile`.

## Trackpoint pairing and the device group

The clone carries the original's `ID_INPUT_TOUCHPAD_INTEGRATION` and
`LIBINPUT_DEVICE_GROUP`. Whether the grabbed original could take the
trackpoint's pairing away from the clone was decided from libinput's source,
for the versions the two releases ship (22.04: 1.20.0, 24.04: 1.25.0, the
latter read from this host's `libinput10 1.25.0-1ubuntu3.7`):

- `tp_pair_trackpoint` (`src/evdev-mt-touchpad.c`, lines 2405–2429 in 1.20.0
  and 2409–2433 in 1.25.0) runs for every touchpad when any device is added,
  and pairs the trackpoint with that touchpad when the touchpad is tagged
  internal and the trackpoint is not USB or Bluetooth. It reads no device
  group. Each touchpad keeps its own `tp->buttons.trackpoint`, so the original
  and the clone are each paired, independently: the clone's top software
  buttons go to the trackpoint through the clone's own pairing, and the
  original, which is grabbed and silent, routes nothing.
- The device group is read in the same file only by `tp_pair_tablet` (declared at line 2573 in 1.20.0 and 2577 in 1.25.0; the group comparison
  is at 2584 and 2588), for left-handed tablet pairing.

So `INPUT_PROP_TOPBUTTONPAD` pads are held, not refused; what the clone needs
is the internal tag, which the carried integration gives it. Source:
https://gitlab.freedesktop.org/libinput/libinput/-/blob/1.20.0/src/evdev-mt-touchpad.c
and the same path at 1.25.0.

The rule hands the two properties to a clone only through
`--clone-properties`, and only when a real touchpad is present right now (not
a clone, accepted by `caps::classify_capabilities`) with the clone's bus type,
vendor, product, and name, and exactly the integration and group the clone's
`phys` claims. A uinput device that merely copies the `phys` gets nothing.
*Held by:* `udev::tests::a_clone_gets_its_properties_only_when_a_matching_touchpad_is_there`.

## What an attacker gains

| Compromise | Gains | Does not gain |
| --- | --- | --- |
| Calls the bus methods as another local user | nothing: refused | — |
| Calls them as the person at the machine | scroll faster or slower, within 0.2–5 | anything else; the interface has no other method and takes no string |
| Takes over the process (a parser bug in the event loop) | the touchpad's events (finger positions, clicks) for as long as it runs, and the one clone it already created, a touchpad: it can move the pointer and click. That includes clicking an on-screen keyboard if one is on screen (GNOME shows one for touchscreen input or when it is turned on in accessibility), at a touchpad's pace, relative motion only | a new uinput device of any kind, a keyboard key, an absolute pointer (the seccomp lock), any other input node (device cgroup), root (dropped, `no_new_privs`, bounding set), the network, the person's files (`ProtectHome`, `ProtectSystem=strict`, another uid), other processes (different uid) |
| Presents a hostile USB device | a pass-through for it only if it looks exactly like a touchpad and reports no key; its stream is parsed as fixed-size `input_event` records by a loop with no allocation driven by the stream | — |

The third row is the residual risk the owner accepted on 2026-11-15. An
earlier draft of this review said it could not be narrowed below "a process
holding `/dev/uinput` can synthesise a keyboard"; that was wrong. uinput
cannot give the descriptor up after creating a device, but the process can
take away its own ability to declare anything more, and it does, before it
reads any input. That blocks new devices only. The one touchpad clone that
already exists is still there, and code that took over the process can still
drive it: move the pointer and click, though not type and not jump to an
absolute position. That includes steering the pointer onto the keys of an
on-screen keyboard and clicking them, which turns into typed text whenever an
on-screen keyboard is shown; it is the residual path from this process to
typed input, and nothing in this design removes it. It is
further bounded by the process having nothing that parses untrusted
variable-length input (events are 24-byte records the kernel produced), by two
methods taking two doubles as its only other input, and by the systemd
sandbox.

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
  tap) is reasoned from libinput's source and its use of the device's
  capabilities, ids, name, and the carried udev properties. Not observed on a
  desktop.
- That `GetSessionByPID` answers "no session" for a program started from the
  application grid, which is why the display-session fallback exists, is
  read from logind's design, not observed here.
- That the coldplug unit's change event makes systemd start each instance is
  read from systemd's handling of `SYSTEMD_WANTS` on a device that newly gains
  the `systemd` tag; the owner run checks that an instance runs right after an
  install from Better Manager, with no reboot.
