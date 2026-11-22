# ADR 0047: The login supervisor ends the greeter's own session, and the text login moves to tty8 and tty9

## Status

Accepted by the owner on 2026-11-22. Proposed the same day for ticket 267,
after the second independent review of the login supervisor. It widens
what ADR 0044 and AGENTS.md let `better-os-login-supervisor` do, which
AGENTS.md says needs a record of its own. AGENTS.md's working rule now
reads as the section "The working rule, on acceptance" below gives it.

## Context

ADR 0044 lets the supervisor start and stop a fixed list of units, run
`plymouth`, reset `/dev/tty1` and `/dev/tty2` and read logind,
`/sys/class/drm` and greetd's cgroup. The review found two things that list
cannot do.

1. **A stop of greetd's unit does not reach the greeter.** `pam_systemd`
   (in Ubuntu's `common-session`) moves the process that opens the greeter's
   PAM session, greetd's session worker, and everything it starts
   (`/usr/sbin/better-os-greeter`, `cosmic-comp`, `better-greeter`) into the
   greeter session's scope, `/user.slice/user-<uid>.slice/session-cN.scope`.
   systemd's stop of `better-os-greeter.service` signals that unit's cgroup;
   its `SIGKILL` at `TimeoutStopSec=4` reaches greetd and nothing in the
   scope; greetd itself sends its session `SIGTERM` only (`context.rs`
   389 to 403 at 0.9.0). A compositor that ignores `SIGTERM`, or any
   compositor once greetd is `SIGKILL`ed, outlives every stop while the
   unit's cgroup reads empty. The next attempt, and in the end the text
   login, would start under a compositor still holding the DRM device and
   VT 1: a frozen frame and a prompt nobody can see, ticket 139's symptom.
2. **logind starts a getty on an empty VT of 1 to 6.** When the screen
   switches to a VT up to `NAutoVTs` (6 by default) that nobody holds,
   logind starts `autovt@ttyN.service`, which is `getty@ttyN.service`. The
   text login on tty2 has `Conflicts=getty@tty2.service`, and a conflict is
   symmetric: the getty logind starts as the supervisor switches to tty2 can
   stop the text login and its message.

## Decision

- **The supervisor ends the greeter's session scope.** It finds the scope by
  fact: the `SCOPE=` of logind's sessions of class `greeter` in logind's
  session files, closing ones included (a session whose leader died is
  `STATE=closing` while its compositor lives), and the cgroup of greetd's
  descendants (which works when logind does not answer). It remembers each
  scope with a process in it until a look sees it empty or gone, at every
  look, with no cap, because once greetd is gone the scope is no longer
  greetd's descendant. 4 s into every stop, the moment systemd kills
  greetd, and at once after a restart of its own that cut a stop short, it
  sends `SIGKILL` to the processes of the remembered scopes and of the
  greeter scopes seen that look. Only a cgroup of the shape
  `/user.slice/user-<digits>.slice/session-<alphanumeric>.scope` is read or
  acted on. At the moment of the kill it reads the scope again and sends
  nothing if any process runs as an account other than root and
  `_better-greeter`: a person's session is never ended this way. Each
  process is reached through a pidfd opened first; its uid and cgroup are
  read from `/proc` after the pidfd is open and the signal goes through the
  pidfd, so a pid reused in between is never signalled.
- **A stop is complete only when none of the greeter's processes holds the
  card.** The unit's cgroup and every remembered scope are empty, and no
  process in the unit's cgroup, in a greeter scope, or running as
  `_better-greeter` holds a `/dev/dri/card*` open, read from the targets of
  `/proc/<pid>/fd` links (`plymouthd` left out, its own path ends it; a
  person's compositor, logind's own copy of a session's device or a
  container given the card is not the greeter's to wait for). Unknown is
  not complete. Only then does another start of the greeter, or the text
  login, follow. A stop that has not completed in 6 s gets the text login
  at once.
- **The text login is on tty8, and tty9 when tty8's does not start.** Both
  are past logind's automatic VTs, so nothing is ever spawned there to
  conflict them out. The supervisor still resets tty1 to text and to
  `VT_AUTO`, so a switch away from a VT left in process mode completes. The
  backstops use tty9. `DeviceAllow=` names `/dev/tty1`, `/dev/tty8` and
  `/dev/tty9`.

## The working rule, on acceptance

AGENTS.md's "Two bounded exceptions for the login path" then reads, for the
supervisor: "it may start and stop only the units its compiled-in list
names, end the greeter session's own scope (`SIGKILL`, through pidfds, to
its root and `_better-greeter` processes during a stop, never a scope with
anyone else's process, ADR 0047), run `plymouth`, reset `/dev/tty1`,
`/dev/tty8` and `/dev/tty9` to text and switch to them, read logind (its
bus and its session files), `/sys/class/drm`, greetd's cgroup and the
processes under it, the greeter's session scopes, which processes hold a
DRM card, and the greeter's account in `/etc/passwd`, and write only
`/var/lib/better-os/boot-watch/` and `/run/better-os/`".

## Consequences

- The supervisor can now end processes of `_better-greeter` beyond
  `plymouthd`. A bug could end a greeter that was about to be ready, which
  costs an attempt and never a person's session.
- Scanning `/proc/<pid>/fd` during a stop reads which processes hold a DRM
  card, and `/proc/<pid>/comm` of every process to leave `plymouthd` out;
  nothing else of them is read or kept, and no descriptor is opened.
- A machine configured with `NAutoVTs` of 8 or more gets autovt gettys on
  the text login's terminals again; this is stated, not handled.
- On a VT owned in process mode by a hung logind, neither the supervisor's
  reset nor the backstops' bounded `chvt` can be shown to complete; ticket
  281 checks it.

## Not chosen

- **Adding the scope to the unit list and stopping it by name.** Its name is
  known only at run time; a list of patterns would be a wider grant than a
  check of each process's account.
- **`KillMode=` or `Delegate=` on greetd's unit.** Neither reaches a cgroup
  `pam_systemd` moved the processes out of.
- **Setting `NAutoVTs=` or `ReserveVT=` in logind's configuration.** It
  changes a system setting for every session on the machine, and the
  package would have to put it back on removal; a VT past the automatic ones
  needs no setting.
