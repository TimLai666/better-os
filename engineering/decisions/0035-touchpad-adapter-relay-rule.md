# ADR 0035: The touchpad adapter's suppression follows the relay rule

## Status

Proposed on 2026-11-21 for ticket 131, for the owner to accept. It widens the
GJS bounds of [ADR 0012](/engineering/decisions/0012-touchpad-gesture-backend.md)
for `adapters/gnome-shell-touchpad/` by applying the relay rule of
[ADR 0018](/engineering/decisions/0018-shared-shell-adapter-extension.md) to it,
which is why it is an ADR and not a commit. The code it describes is built and
tested on the ticket 131 branch.

## Context

Ticket 131 found GNOME's own three-finger swipe gestures turned off on the
owner's machine while no Better OS gesture worked. Two facts made that
possible:

- The extension took `SuppressBuiltInGestures(true)` from any caller on the
  session bus and held it until it was disabled. A gesture service that
  stopped, crashed, or was killed with its session left GNOME's trackers off
  for the rest of the login.
- After an upgrade the shell keeps running the extension it loaded at login,
  so the old extension's suppression outlives the old service too.

ADR 0018 already solved the same problem for the shared adapter: a relay acts
only while its Rust counterpart owns a named bus name, answers only that
name's owner, accepts the owner only when its process runs the packaged
service, and puts GNOME's behaviour back the moment the name goes. ADR 0018's
last section already described the touchpad adapter moving under that rule
with `better-touchpad-gestured`'s own bus name as the counterpart.

## Decision

**The touchpad adapter's one change to GNOME, turning its swipe trackers off,
follows ADR 0018's relay rule in ADR 0018's exact form. Its counterpart is the
gesture service's bus name `org.betteros.TouchpadGestures1`, accepted only
when the owner's process runs `/usr/bin/better-touchpad-gestured`.** The
extension stays a separate extension in its own package; it does not move into
`shell-adapter@betteros.org`.

What the JavaScript may now do, in addition to ADR 0012 and ADR 0015:

- Watch `org.betteros.TouchpadGestures1`, and accept the name's owner as the
  counterpart only when `GetConnectionCredentials` names a process whose
  `/proc/<pid>/exe` is `/usr/bin/better-touchpad-gestured`. That link is the
  one thing it reads beyond its interface XML, once per new owner.
- Keep the counterpart's unique name, beside its bus handles and whether the
  trackers are off. Nothing else is kept.
- Serve `SuppressBuiltInGestures` in GJS's `…Async(params, invocation)` form:
  `true` is refused with `org.freedesktop.DBus.Error.AccessDenied` unless
  `invocation.get_sender()` is the accepted counterpart; `false` is taken from
  anyone, because it only returns GNOME to its own behaviour.
- Put the trackers back by itself the moment the counterpart's name loses its
  owner, and on `disable()`.
- Put them back by writing `enabled = true`, the value GNOME keeps when
  nothing is under way, never a value captured when suppressing (which could
  be GNOME's own `false` from the middle of a workspace animation).

What it still may not do is everything ADR 0012 and ADR 0015 forbid: no
threshold, cooldown, conflict decision, configuration, or decision of any kind;
nothing stored beyond the above; nothing executed; no free-text argument. It
does not judge whether to suppress. The gesture service decides, and only for a
gesture that collides, is switched on, verified, and only after a gesture has
arrived from the adapter.

The gesture actions (`ShowOverview`, `ShowDesktop`, `SwitchWorkspace`) stay
open to any caller, as ADR 0012 has them: they do only what the person can do
with a key, and gating them is a separate decision. The service itself accepts
gesture signals only from the adapter's current unique name.

### What the owner check does and does not stop

`/proc/<pid>/exe` proves which file a process is running, not what that
process does. The check therefore stops an owner that is accidental or
unrelated: another program that happens to take
`org.betteros.TouchpadGestures1`, a test build run by hand from a working
tree, a leftover copy somewhere else on disk. Those are never obeyed.

It does not stop hostile code running as the same user. Such code can run the
genuine `/usr/bin/better-touchpad-gestured` with `LD_PRELOAD`, point the
genuine binary at another `XDG_CONFIG_HOME` holding a configuration of its
choosing, bind another file over that path in a mount namespace of its own,
or simply stop the real service first and take the name. Every one of those
passes the check.

What such code gains is bounded by what the extension can do with it: the
worst outcome is GNOME's own swipe gestures turned off until the name's owner
leaves the bus, which the extension then undoes by itself. The gesture actions
the extension serves are open to any caller already (ADR 0012), and nothing
here executes anything. The same user can do that much and more through the
person's own session already, so the residual risk is accepted, as ADR 0018
accepts its own. A program holding the name also keeps the real service out:
it exits with "another process already owns org.betteros.TouchpadGestures1"
until systemd's start limit, which leaves Better OS gestures off and GNOME's
own on.

The check runs once, when the name gets a new owner. A service already
accepted stays accepted after its file is replaced by an upgrade, until it
leaves the bus; a new owner, or the extension being enabled again (a login, a
shell restart), is checked again. An owner whose program file was replaced or
removed before it took the name has a link ending in `(deleted)` and is
refused.

### GNOME does not let go reliably

Recorded because it limits what any suppression can promise on GNOME 46,
read from `libshell-14.so`'s `js/ui/*.js` on 2026-11-21:

- `workspaceAnimation.js` writes its own tracker's `enabled`: `false` when an
  animated workspace switch starts and `true` when it ends, and `true` when the
  overview hides. So after the first animated switch GNOME's three-finger
  workspace swipe answers again beside a Better OS gesture on the same fingers.
- `workspacesView.js` and `appDisplay.js` have two more three-finger trackers,
  the overview's workspace strip and the application grid's pages, which work
  only inside the overview. The adapter never turns them off.
- Holding the trackers off against GNOME's own writes would mean reacting to
  every change of the property, which is the extension making a decision.
  This ADR does not grant it.

So a Better OS gesture that collides with one of GNOME's three-finger swipes
can do both things at once. The Touchpad page says so when such a gesture is
on, the shipped Mac-style preset collides with nothing (it swipes with four
fingers, which GNOME 46 ignores), and nothing in Better OS may describe
suppression as reliable.

## Consequences

- `crates/touchpad-session/tests/extension.rs` asserts the counterpart watch,
  the executable check in ADR 0018's form, the sender check on suppression,
  the restore on the name going and on `disable()`, the restore writing
  `true`, and the field list (`_capturedId`, `_counterpart`, `_exported`,
  `_nameId`, `_suppressed`, `_watchId`).
- `crates/touchpad-gesture-service/tests/pipeline.rs` asserts on a private bus
  that the service calls as its own name's owner and that a forged gesture
  signal from another connection is ignored.
- `AGENTS.md`'s language rule names this ADR as proposed beside ADR 0018.
- If the owner rejects it, the extension goes back to taking suppression from
  anyone, and the stale-suppression defect of ticket 131 comes back with it.
