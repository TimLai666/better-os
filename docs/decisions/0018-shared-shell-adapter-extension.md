# ADR 0018: A shared Better OS GNOME Shell extension that only relays

## Status

Proposed on 2026-11-14 for ticket 106. The project owner chose the approach
(an extension that only relays, with a Rust service that decides) and its
placement (a shared extension in its own package) the same day; this record
is for the owner to accept. It widens the GJS exception of
[ADR 0012](0012-touchpad-gesture-backend.md) and
[ADR 0015](0015-touchpad-adapter-reports-the-device.md) to one more directory,
which is why it is an ADR and not a commit.

## Context

GNOME Shell turns every window's request for attention into a "“…” is ready"
banner and a notification. The handler is `WindowAttentionHandler` in
`js/ui/windowAttentionHandler.js` (read from GNOME Shell 46.0's
`libshell-14.so`), connected in its constructor to `global.display`'s
`window-demands-attention` and `window-marked-urgent`. Nothing outside the
shell's own process can change what it does. Ticket 106 asks for a choice per
application among four outcomes, set in Better Manager.

ADR 0012 allows JavaScript only in `adapters/gnome-shell-touchpad/`, and only
to report what the compositor saw and to perform what the shell already
performs: no configuration and no decision. A per-application choice is both.
Two ways were open: let an extension read a settings schema and apply it
(configuration and decisions in JavaScript), or let it relay and decide in
Rust. The owner chose the second.

Later relays are expected: ticket 94's launcher panel and ticket 95's top bar
also need the shell's own process. One shared extension that each feature adds
a relay to is cheaper and easier to bound than one extension per feature.

## Decision

**Better OS ships one shared GNOME Shell extension, `shell-adapter@betteros.org`,
in `adapters/gnome-shell-adapter/`, packaged alone as `better-shell-adapter`.
It is made of relays. Each relay is active only while its Rust counterpart
owns a named session bus name, answers only that name's current owner, and
does nothing otherwise, leaving GNOME's own behaviour exactly as GNOME set it
up. The extension stores nothing and decides nothing.**

### What the JavaScript may do

- Report, as a signal, what the shell saw: values read from a shell object at
  the moment of the event, copied unchanged; and, for a window it reported,
  that the window's request ended (focused, closed, or withdrawn), through
  signal handlers on that window tagged with the extension and removed when
  they fire or when GNOME's handler is put back.
- Read one file: the relay's interface XML beside it, once, at enable. And
  read one link: `/proc/<pid>/exe` of the process the bus names as the
  counterpart's owner (`GetConnectionCredentials`), to compare it with one
  fixed path.
- Perform an action GNOME Shell already performs, named as itself, on an
  object the shell already has, identified by the shell's own id.
- Take one of GNOME's own handlers off the signal it is connected to and put
  it back exactly as GNOME connects it, only when the counterpart asks and
  only while the counterpart owns its name. It puts it back by itself when the
  name goes, and on `disable()`.
- Watch the counterpart's bus name, and accept the name's owner as the
  counterpart only when its process runs `/usr/libexec/better-window-attention`,
  the packaged service. Every method refuses
  (`org.freedesktop.DBus.Error.AccessDenied`) a caller that is not that
  accepted owner: `invocation.get_sender()` must equal it.
- Send its signals to the accepted owner alone, never broadcast, so no other
  program on the session bus learns which windows asked for attention.

### What it may not do

- Read any file but its interface XML, or read or write any setting or schema;
  keep any list, map, or record beyond its bus handles, the counterpart's
  unique name, and whether the takeover is on.
- Choose between outcomes, compare an application against anything, run a
  timer, or interpret what it reports.
- Spawn or execute anything. Take free text in any method.

### The window attention relay

Interface `org.betteros.ShellAdapter1.WindowAttention` at
`/org/betteros/ShellAdapter1/WindowAttention`, bus name
`org.betteros.ShellAdapter1`. The extension loads it from
`org.betteros.ShellAdapter1.WindowAttention.xml` beside it. Counterpart:
`org.betteros.WindowAttention1`.

| Member | Shape | What it is |
| --- | --- | --- |
| `WindowAttention` signal | `(t window, s app_id, s app_name, u kind, b focused, b skip_taskbar)` | The window's `get_id()`, `Shell.WindowTracker`'s application id and name for it, 0 for `window-demands-attention` or 1 for `window-marked-urgent`, and the window's own `has_focus()` and `is_skip_taskbar()`. Emitted only while GNOME's handler is off. |
| `WindowAttentionEnded` signal | `(t window, u reason)` | A reported window stopped asking: 0 focused, 1 closed (`unmanaged`), 2 withdrawn (`demands-attention` and `urgent` both false). Once per report. |
| `SuppressBuiltInAttention(b)` | in `b` | `true` takes `Main.windowAttentionHandler` off the two display signals and connects the reporting handler; refused with an error while the counterpart is absent or the handler is not where GNOME 46 keeps it. `false` puts GNOME's handler back. |
| `ShowAttentionBanner(t)` | in `t` | Runs GNOME's own handler for that window: its banner, notification, and notification policy. |
| `ActivateWindow(t)` | in `t` | `Main.activateWindow`, what clicking the window in the taskbar does. |
| `ClearAttention(t)` | in `t` | `Meta.Window.unset_demands_attention`. |
| `Capabilities()` | out `s` | A JSON report: protocol version, shell version, relays, and whether the counterpart is present, the takeover is on, and GNOME's handler was found. |

Every method but `Capabilities` is served in GJS's `…Async(params,
invocation)` form and refuses any caller but the counterpart's current unique
name; each window method does nothing when the id names no window.

**What the owner check does and does not stop.** Taking a session bus name
needs nothing but a session bus connection, so without the check any program
the person runs could own `org.betteros.WindowAttention1` before the service
and drive `ActivateWindow` and the takeover. The check closes that: the owner
must be the packaged binary, read from the kernel's own record of the process,
not from anything the owner says. What it does not stop is a program running
as the same user that can already control that process, for example by
`ptrace` or by rewriting its memory, or by replacing `/usr/libexec` (which
needs root). A program with those powers can already do far more than move a
window forward, so this residual risk is accepted. While a service from a
removed or upgraded package still runs, its link ends in `(deleted)` and it is
refused until the package's postinst restarts it.

The extension's `session-modes` are `user` and `unlock-dialog`, so a request
made while the screen is locked still follows the choice instead of falling
back to GNOME's banner over the lock screen. The relay holds nothing a lock
screen could expose: it reports a window id, an application id and name, and
performs the same four actions.

### The service that decides

`better-window-attention` (crate `window-attention-service`) owns
`org.betteros.WindowAttention1`, asks the extension to take over when the
extension is on the bus (and again whenever it reappears, as after a shell
restart), and answers each request with `window_attention_core::decide` from
the choices Better Manager writes to
`$XDG_CONFIG_HOME/better-os/window-attention/choices.json`. It records each
application that asks in `seen.json` beside it. Each file has one writer.
It believes a signal only when the message's sender is the extension's current
unique name (from `GetNameOwner` and `NameOwnerChanged`), because a signal sent
straight to the service skips the bus's match rules. Better Manager calls its
`ChoicesChanged()` after saving, and the service takes back every taskbar mark
the new choices no longer ask for. It asks the extension to take over with a
backoff of up to 30 seconds, and again each time the extension reappears.

The service ships in the `better-manager` package, not in
`better-shell-adapter`. Better Manager enables and starts a component's user
units from its own record (`manager-core`'s user unit pass, which reads units
from the component's dpkg file list). `better-shell-adapter` is a dependency
package with no catalog record, so a unit in it would never be enabled by
anything; the window attention page is Better Manager's, so its service lives
with that window. The unit is `Type=dbus`, wanted by
`graphical-session.target`, and D-Bus activatable.

**What starts it.** It is an always-on unit, with no off switch of its own:
the default choice, the taskbar mark only, already differs from GNOME's. The
package's postinst (`packaging/maintainer/user-units.sh`, the same helper
ticket 105 built) enables it for every user with `systemctl --global enable`,
so each login starts it, and starts it for everyone already logged in, on a
first install and on every upgrade, whether apt or Better Manager's
privileged service installed the package. Before this, nothing started it
until the person opened Better Manager, whose startup pass enables a
recorded component's units: the postinst only enabled a new unit for someone
who already had the component's other units enabled, and `better-manager` had
none. Removing `better-manager` runs its prerm, which disables and stops the
unit in every logged-in user's manager and takes the global enablement away,
and the extension gives GNOME its handler back as the name goes. On SIGTERM
or SIGINT the service first sends `urgent: false` for every taskbar mark it
holds.

### The adapter travels with Better Manager's own update

Better Manager updates itself through the privileged service, which installs
the verified `.deb` files a plan names with `--no-install-recommends`, and the
adapter is in no APT source. So the adapter is a **companion** of the manager's
artifact, and this decision adds one optional field to the manifest schema and
to the privileged service's plan, an additive change to [ADR
0007](0007-privileged-daemon-ipc.md)'s protocol version 1:

- A manifest artifact may carry `companions`: each a first-party package
  (`better-` prefix), its HTTPS URL, its SHA-256, and its release asset, which
  must be `<package>_<the manifest's version>_ubuntu-<release>_<arch>.deb`. At
  most four, none repeated, none the component itself. A manager from before
  this field ignores it.
- A plan step may carry `companions`, each a package name and an artifact. The
  shared contract checks the same naming at the step's target version, and a
  removal carries none. Without companions the step is written exactly as
  before, so every other plan reads the same to any privileged service.
- The privileged service refuses a companion that is not a Better OS package,
  checks each one's cached bytes and control fields as it checks the step's
  own package, records a rollback point for each, installs the step's package
  and its companions in **one** `apt-get install`, records each installed
  artifact, and health-checks each. Either failing fails the step, and both
  are undone.
- A privileged service from before this refuses the unknown field before
  running anything. The manager then sends the same plan without companions:
  its own update still succeeds, as it did before, and the adapter is what
  `install.sh` or a later update brings.

`better-manager` **recommends** `better-shell-adapter` and does not depend on
it, because that older path installs the manager's `.deb` alone. The manifest
can name the companion only from the release that first publishes the adapter,
with that release's checksums, in the same step that writes the manager's own
(`docs/release-packaging.md`). Without the adapter the Window attention page
says it is missing and GNOME keeps its own behaviour.

### The taskbar mark

The ticket assumed that keeping the window's request keeps the taskbar's mark.
On Zorin OS 18 it does not. Zorin's taskbar (`zorin-taskbar@zorinos.com`, a
Dash to Panel derivative) draws its mark from two sources only
(`notificationsMonitor.js`): a notification from GNOME's
`WindowAttentionSource`, and the Unity launcher API's
`com.canonical.Unity.LauncherEntry.Update` signal with `urgent: true`. It
never reads a window's `demands-attention`. So "Taskbar mark only" keeps the
request on the window and has the **service** emit
`Update("application://<app_id>", {"urgent": true})`. The service remembers
which windows it marked and sends `urgent: false` for an application once its
last marked window is focused, closes, or withdraws its request
(`WindowAttentionEnded`), when the saved choice no longer asks for the mark,
and when the extension goes away. The taskbar also clears its mark when the
application is focused, and mutter clears the window's request on focus.

### How the touchpad adapter could move in

`touchpad-adapter@betteros.org` stays as it is. It could become a second relay
of this extension: its interface would move to
`org.betteros.ShellAdapter1.Touchpad` under the same bus name, its counterpart
would be `better-touchpad-gestured`'s own bus name (which it does not own
today and would have to), its swipe trackers would be suppressed only while
that name is owned, and `better-touchpad` would depend on
`better-shell-adapter`. ADR 0015's device-node bound and its test would move
with it unchanged. A Better Touchpad that still talks to the old name during
an upgrade is the compatibility case that move would have to solve.

## Consequences

- `AGENTS.md` names both directories as the only places GJS is allowed.
- `crates/window-attention-service/tests/extension.rs` asserts the bound
  against the file as an **allow-list**, not a list of forbidden words: the
  module's top level may hold only imports, one string or number constant per
  line, and the class; the class may define only a fixed set of methods,
  assign only five fields (`_exported`, `_nameId`, `_watchId`, `_counterpart`,
  `_suppressed`), declare only a fixed set of locals, test only a fixed list
  of conditions, compare only on those lines and two capability facts,
  construct only `TextDecoder` and `GLib.Variant`, and hold no collection
  literal. A second test applies variants that keep a list of quiet
  applications or remember what asked, and requires each to fail. The same
  file asserts that the interface declares exactly the members
  `window_attention_core::bus` names, that no input argument is anything but
  `t` or `b`, that every method checks the caller against the counterpart's
  unique name, that GNOME's handler and the window handlers are removed when
  the counterpart's name goes and on `disable()`, that the one file read is
  the interface XML, and that `gjs` parses the module.
- `crates/window-attention-service/tests/session_bus.rs` sends a forged
  `WindowAttention` straight to the service from another connection and
  requires it to be ignored.
- A new relay adds an interface file, its members to a Rust contract module,
  and its own assertions in the same shape, and needs this ADR amended.
- GNOME's handler is back as soon as either the extension or the service is
  gone. The uuid stays in the person's `enabled-extensions` list after the
  package is removed, as it does for any extension a package removes; GNOME
  ignores a uuid it cannot find.
- On GNOME 42 (Ubuntu 22.04) the extension does not load, because its
  metadata names 46 only; Better Manager says the shell is not supported
  rather than offering to turn it on.

## What was verified, and how

GNOME Shell 46.0 as `gnome-shell --headless --wayland --no-x11
--virtual-monitor 1280x800` on a private `dbus-run-session`, with `HOME`, every
`XDG_*` directory, and dconf in a temporary directory, the extension linked
into that home, and Zorin's taskbar enabled. A GTK 4 test window under its own
desktop id asked for attention through its own `present()` while another
window covered it (mutter sets the request only for an obscured window). All
four choices, a per-application override and its removal, the extension
disabled and re-enabled, and the service stopped were each observed through
the shell's state, the taskbar's own state, and a screenshot; ticket 106
records the results.
