# ADR 0044: Better OS owns the login and lock path: greetd, a login supervisor with deadlines, and a GNOME lock without GDM

## Status

Proposed on 2026-11-21 for ticket 259 (`design/login-screen-spec.md`,
decisions L1, L2, L7 and L8), and revised the same day for the owner's two
requirements: locking must never depend on GDM, and a boot must never be
left without a login screen. Accepted by the owner on 2026-11-21, who answered L1 (B), L2 (a) and L8 (a)
as recommended; L7 is still open. It does not
change ADR 0038's C3 by itself: GDM stays the login screen until
the fifth step of the design's section 9, and then only for a person who
chooses otherwise. ADR 0033 is not
edited; on the Better OS path its watch is replaced by the supervisor below.

## The owner's decisions

| # | Decision | Recommendation |
| --- | --- | --- |
| L1 | Who owns the login and lock path. (A) GDM keeps login; Better OS's lock screen only in the Better OS session. (B) Better OS owns it on greetd: its greeter, a login supervisor, and a GNOME unlock service; GDM installed and not running. (C) As B with a display manager Better OS writes itself | **(B)**, in the order below; making it the default for everyone is a later decision |
| L2 | How GNOME locks without GDM: (a) a service answering GNOME Shell's reauthentication calls, (b) a lock on another virtual terminal, (c) GDM running for it, (d) GNOME not offered | **(a)** |
| L7 | The greeter's compositor: (a) `cosmic-comp`, (b) Cage | **(a)** |
| L8 | The last fallback when the greeter cannot appear: (a) a text login that says what failed, (b) GDM first | **(a)** |

## Context

The owner asked on 2026-11-21 for a Better OS login screen laid out like
Luma's, in Rust, and then set two requirements: "Solve the screen-lock
problem: we must still be able to lock the screen, but not by relying on
GDM" and "the earlier problem where GDM sometimes cannot start (the login
screen never appears): we must not have that problem."

ADR 0038's C3 kept GDM because it lists any session and a broken greeter
would keep the person out of both GNOME and the Better OS session. Ticket
139 is why that weighs: on the owner's machine GDM has started and never
opened its greeter, once for 16 minutes; the cause is unknown; ADR 0033's
watch restarts it once. Ticket 256 adds a logind crash at logout that left
no greeter.

Facts found for this record (sources in the design's "Sources"):

1. **GNOME needs an answer from `org.gnome.DisplayManager`, not GDM
   itself.** GNOME Shell 46 creates its screen shield only if the `Version`
   property of `org.gnome.DisplayManager.Manager` answers
   (`loginManager.js` lines 38 to 54, `main.js` lines 230 to 231), and checks
   an unlock password by `OpenReauthenticationChannel(username)` and a
   private `org.gnome.DisplayManager.UserVerifier` connection
   (`gdm/util.js` line 505, `libgdm/gdm-client.c`, `daemon/gdm-session.xml`).
   GDM serves this even for a session it did not start, through a temporary
   reauthentication channel (`gdm-manager.c` lines 1155 to 1235). Mutter 46
   has no `ext_session_lock_v1`, so nothing outside the shell can lock it.
2. **GDM cannot run beside another display manager**: its local display
   factory always manages `seat0`.
3. **GDM leaves the splash up when it is active and silent**:
   `gdm.service` quits Plymouth itself and only `OnFailure=` makes systemd
   do it; and GDM 46 waits 10 seconds for `seat0` to become graphical and
   then forces an X11 attempt (`gdm-local-display-factory.c` lines 55, 869
   to 894). Neither is shown to be ticket 139's cause.
4. **greetd runs one session at a time** and ends when its greeter exits
   without starting a session (`context.rs` at 0.9.0); its unit gives up
   after five restarts in 30 seconds.
5. **GDM stays installed on Zorin OS**: `zorin-os-desktop` depends on
   `gdm3`; `gnome-shell` depends on `gir1.2-gdm-1.0` (the client library,
   not the daemon).
6. **Debian's display manager switch** is the debconf question
   `shared/default-x-display-manager`, and every registered package's
   postinst relinks `display-manager.service` from
   `/etc/X11/default-display-manager` (`gdm3.postinst` lines 26 to 58);
   greetd's own package does not take part, `cosmic-greeter` does.

## Decision

**Who owns the path (L1, B).** While the Better OS login screen is chosen,
the login and lock path is Better OS's and uses nothing of GDM:

- **Login:** `greetd` 0.9.0 from Ubuntu's archive, run by Better OS's unit
  `better-os-greeter.service` with `cosmic-comp` (L7) and `better-greeter`,
  as `_better-greeter`. greetd is kept rather than replaced (C): it already
  does the VT, PAM and session start in about 2,800 lines of Rust, its
  GPL-3.0-only licence combines with Better OS's GPL-3.0-or-later, and what
  C would add (several sessions, VT-switch locking) is not needed, and in
  the second case not safe (fact 1 and the design's section 5.6).
- **Lock in the Better OS session:** `better-lock` on `cosmic-comp`'s
  `ext_session_lock_v1`.
- **Lock in GNOME (L2, a):** `better-os-gnome-unlock`, a root service that
  owns `org.gnome.DisplayManager` only while the Better OS greeter is the
  display manager (`Conflicts=gdm.service`), answers `Version`,
  `RegisterSession` and `OpenReauthenticationChannel`, and runs PAM for the
  caller's own user on a private connection, after checking from the bus's
  credentials that the caller is GNOME Shell in a local session of that
  user. GNOME Shell is not patched. The greeter lists GNOME only while the
  service answers.
- **The login screen always appears (L8, a):**
  `better-os-login-supervisor`, a root service, watches every stage from
  the display manager's start to a prompt accepting input with a deadline
  each, owns every retry (one plain, one safe mode), quits Plymouth in every
  path, and when the greeter cannot appear starts a text login on the first
  virtual terminal that names the failed stage. The property: within 60
  seconds of `better-os-greeter.service` starting, the Better OS login
  screen or a text login is accepting input, Plymouth has quit, and the
  journal names every stage that failed. GDM is never started on its own.
- **Privileged code.** The supervisor and the unlock service are new system
  services in the `better-os-greeter` package; AGENTS.md's rule that
  privileged code lives in `manager-daemon` is extended to them by this
  record, as ADR 0030 did for the touchpad pass-through, each with the
  limits in `engineering/specs/login-screen-security.md`.

**The order** (the design's section 9), each step proven before the next:

1. the lock screen in the Better OS session (no change to booting);
2. GNOME locking through the unlock service, in the VM, under greetd with a
   test greeter;
3. the supervisor's property in the VM with every injected failure of the
   design's section 5.10 (no DRM device, a late one, a seat that is not
   graphical, the compositor and the greeter crashing, the greeter hanging,
   logind restarting, the greeter's account missing or locked, a full disk,
   Plymouth hanging);
4. the Better OS greeter, on both bases, and the way back to GDM from a
   text console done by hand;
5. offered as a choice in Better Manager, first on the owner's machine for
   a week with every supervisor record read;
6. whether it becomes the default for new installs or for everyone is not
   decided here: the owner decides it after step 5, in a new record.

**Switching on is one action** in Better Manager, carried out by
`manager-daemon`:

1. capture, into `/var/lib/better-os/login-screen/previous.json` (root,
   0644): the content of `/etc/X11/default-display-manager`, the target of
   `/etc/systemd/system/display-manager.service`, and the debconf value of
   `shared/default-x-display-manager`;
2. install `greetd` and `better-os-greeter` if they are not installed, and
   record which of them this step installed;
3. set the shared question to `better-os-greeter` and run
   `dpkg-reconfigure --frontend noninteractive better-os-greeter`, which
   writes the file and relinks the alias;
4. check: the file names `/usr/sbin/better-os-greeter`, the alias points at
   `better-os-greeter.service`, `systemd-analyze verify` accepts the units,
   the TOML names `_better-greeter` and an executable command, and the
   account exists; if any fails, put back what step 1 captured and report
   the failure;
5. take effect at the next boot (or when the person presses Restart now);
   the running session is never stopped by the switch.

**Switching off is one action** and puts back exactly what step 1
captured: the debconf value, the file's content, and the link's target;
`dpkg-reconfigure` of the captured package relinks; the supervisor and the
unlock service are stopped before GDM starts; then the packages step 2
installed are purged (`greetd` only if nothing else depends on it), and
`previous.json` is removed. If the captured display manager is no longer
installed, GDM is chosen and the page says so. Removing the Better OS
session switches the login screen off first.

**The way back by hand** is the numbered steps in
`engineering/specs/login-screen-security.md`, shown next to the switch and
named in the text fallback's message.

## Consequences

- With L1 B, the person's machine depends on Better OS for login and lock:
  three root programs on the path (greetd, Ubuntu's; the supervisor and the
  unlock service, Better OS's), each reviewed.
- GNOME keeps its own lock screen; only the password check moves from GDM's
  worker to the unlock service. Each `gnome-shell`, `libgdm1` or
  `gir1.2-gdm-1.0` update Ubuntu or Zorin ships is re-checked (ticket 280);
  the service refuses to start on a `libgdm1` major version it was not
  tested with, and the greeter then stops listing GNOME rather than offer a
  session that cannot lock.
- A second person cannot log in while one is logged in (greetd).
- The greeter depends on the session's closure (`cosmic-comp`), so it is
  offered only where the Better OS session is installed, on Ubuntu 24.04
  bases.
- Ticket 139's cause stays unknown; on the Better OS path it can no longer
  leave the machine on the splash, but the design does not claim to fix it,
  and a fault before the display manager starts is outside it.

## Alternatives not taken

- **A (GDM keeps login).** Fails both requirements.
- **C (a display manager of Better OS's own).** More root code for no gain
  over greetd plus the supervisor; its one capability greetd lacks, several
  sessions, is not needed.
- **Locking GNOME on another virtual terminal.** Switching back to the
  session's terminal shows its content unless GNOME's own lock hides it.
- **GDM running only for GNOME's lock.** Not possible beside another display
  manager, and a dependence on GDM.
- **GDM as an automatic fallback (L8 b).** GDM would be back on the path the
  owner asked not to rely on, and a silent GDM (ticket 139) would need its
  own deadline behind it.
- **`cosmic-greeter` as the login screen.** Not Better OS's design, and its
  root daemon reads every account's COSMIC configuration from their homes
  before anyone logs in (`daemon/src/main.rs`).
- **Restyling or patching GNOME Shell or GDM.** ADR 0012 does not allow
  Better OS to write GNOME Shell code, and a patched shell is a fork to keep.
- **Cage as the compositor.** One more package, no layer shell, one window
  across all outputs (`cage.c` at v0.1.5).
- **Writing `display-manager.service` directly without debconf.** Every
  `gdm3` upgrade would relink it from `/etc/X11/default-display-manager`.

## Amendment of 2026-11-22: corrections after an independent review

An independent review of the design found faults in how the decision above
is carried out. None changes the owner's answers (L1 B, L2 a, L8 a) or the
two requirements; each corrects a mechanism. Where a sentence above and this
section disagree, this section holds. The design
(`design/login-screen-spec.md` sections 0, 3.6, 4.5, 5.1, 5.2, 5.5, 5.6,
5.10, 6) and the security review carry the details.

1. **The supervisor is the display manager.** Above: "within 60 seconds of
   `better-os-greeter.service` starting", and the design had the supervisor
   `PartOf=` and `Before=` the greeter's unit. `PartOf=` propagates a stop
   or restart of the greeter's unit to the supervisor (systemd.unit(5)), so
   every retry and the fall back would have stopped or reset it. Now
   `display-manager.service` is an alias of
   `better-os-login-supervisor.service` (`daemon_name`
   `/usr/sbin/better-os-login-supervisor`); the supervisor starts
   `better-os-greeter.service` itself after stages 1 to 3 and depends on
   nothing it controls; it keeps its state in `/run`, so a restart
   continues; it has `Restart=always` with a start limit (a watchdog alone
   only marks a unit failed, systemd.service(5)).
2. **A backstop independent of the supervisor:**
   `better-os-login-backstop.service`, standard programs only, started by
   the supervisor's `OnFailure=` and by a timer 90 seconds after its first
   start, gives `plymouth quit` and a text login on tty2 when the greeter
   never became ready.
3. **The figure is 80 seconds, not 60** (90 when the supervisor itself
   fails): the 60 counted stopping a unit as free. Every stop now has a
   4-second `SIGTERM` limit and `SIGKILL`; the fall back moves to tty2
   rather than wait for a process the kernel cannot kill, and such a
   process is stated as a limit, not promised away.
4. **The greeter's unit is not Debian's `greetd.service`:** no
   `After=plymouth-quit-wait.service` (it would wait for a splash only the
   supervisor quits), `TimeoutStopSec=4`, `Restart=no`, and no `OnFailure=`
   to a text login (it raced the supervisor's attempts on tty1); only
   `OnFailure=plymouth-quit.service`.
5. **The supervisor's sandbox** closes the network with `IPAddressDeny=any`,
   not `PrivateNetwork=`, which would hide `plymouthd`'s abstract socket;
   it does not repair accounts at boot.
6. **Switching off, removing and upgrading never stop a running display
   manager.** Above: "the supervisor and the unlock service are stopped
   before GDM starts; then the packages step 2 installed are purged".
   greetd ends the running session when it is stopped (`server.rs` lines
   261 to 273, `context.rs` lines 389 to 403 at 0.9.0), and stopping the
   unlock service would leave a locked GNOME session without a password
   check. Now switching off changes only the debconf value, the file and
   the link, and takes effect at the next boot; Better Manager's first start
   after that boot purges what the switch installed. Upgrades never stop or
   restart `better-os-greeter.service`.
7. **The GNOME unlock service's boundary is the caller's own account.**
   Above: "after checking … that the caller is GNOME Shell". The executable
   check stays as a filter, not a boundary (the same person can run
   `/usr/bin/gnome-shell`). Its socket is 0666 in a 0711 directory, as
   GDM's, matched by peer credentials (the 0700 directory of the design
   would have refused the person's own shell). It is `Type=dbus`, ships its
   own bus policy (no dependence on `gdm3`'s file), limits failures per uid,
   runs each PAM conversation on its own thread with a time limit, keeps
   serving running sessions when `libgdm1` changes (above: "refuses to
   start"), and the greeter checks it right before starting GNOME, because
   GNOME Shell asks only once, at start.
8. **The lock is not a boundary against the person's own programs.** logind
   lets a session's own user `Unlock` it without a password (systemd v255
   `logind-session-dbus.c` lines 201 to 221), and a program of the person's
   can take `cosmic-comp`'s lock after killing `better-lock`. Both GNOME and
   the Better OS session have this limit; it is stated, and closing the
   compositor half would need a patch under ADR 0039.
9. **The greeter does not write AccountsService.** Another account's record
   needs an administrator's password; a small program in the new session
   writes the person's own choice instead.
10. **GDM's automatic login is carried over** into greetd's
    `initial_session` by the switch, and GDM's file is never edited.
11. **AGENTS.md records the exception** this decision makes to "privileged
    system mutation … belongs in `manager-daemon`", as a working rule.
