# ADR 0044: When the Better OS login screen replaces GDM, and how switching stays one action each way

## Status

Proposed on 2026-11-21 for ticket 259 (`design/login-screen-spec.md`,
decision L1, with L2 and L7 that depend on it). Accepted when the owner
answers L1. It amends nothing already accepted: ADR 0038's C3 ("Keep GDM")
stays true under the recommended answer until the owner chooses B, and ADR
0033's watch gains a greetd form (ticket 267) only if B or C is chosen.

## The owner's decisions

| # | Decision | Recommendation |
| --- | --- | --- |
| L1 | (A) The lock screen first, inside the Better OS session only; GDM keeps doing login. (B) The greeter offered as a choice in Better Manager once the session is installed. (C) The greeter by default once the Better OS session is the default (S8). | **(A) now, then (B)** once tickets 262 to 268 pass on both VMs and the lock screen has run a week on the owner's machine. **Not (C)** while GNOME is still offered at login |
| L2 | With B or C, GNOME in the greeter's session list: (a) listed plainly, (b) listed with the line that it cannot lock there, (c) not listed | **(b)** |
| L7 | With B or C, the greeter's compositor: (a) `cosmic-comp`, (b) Cage | **(a)** |

## Context

The owner asked on 2026-11-21 for a Better OS login screen laid out like
Luma's, in Rust. ADR 0038 decided C3 the same day: GDM stays, because "it
lists any Wayland session, and ADR 0033's watch keeps working", and a broken
greeter would keep the person out of both GNOME and the Better OS session.
Ticket 139 is the reason that weighs: the owner's machine has had boots on
which GDM started and never showed its login screen, once for 16 minutes,
with the cause still unknown; ADR 0033's watch treats that symptom for GDM
and only for GDM. Anything that replaces GDM must be at least as easy to get
out of.

Four facts found for this record (sources in the design's "Sources"):

1. **GNOME cannot lock its screen without GDM.** GNOME Shell 46 makes its
   screen shield only when `org.gnome.DisplayManager` answers on the system
   bus (`js/misc/loginManager.js` lines 38 to 54, tag 46.0); Luma had to
   patch GNOME's mobile shell to run under greetd at all
   (`patches/gnome-mobile/0003`, `0009`). Mutter 46 offers no
   `ext_session_lock_v1` (`libmutter-14.so.0`'s strings), so Better OS's
   lock program cannot lock a GNOME session either. A GNOME session started
   by any other greeter has no lock screen. Read, not run.
2. **greetd runs one session at a time** on its VT and shows the greeter
   again only when that session ends (`greetd/src/context.rs` at 0.9.0).
   Under GDM a second person can log in while the first is logged in.
3. **A greeter that exits without starting a session ends greetd**
   ("greeter exited without creating a session", `context.rs` lines 357 to
   360); systemd restarts it five times in 30 s and then gives up
   (Debian's `greetd.service`). Without more, a crashing greeter is a
   machine at a text console.
4. **Debian's display manager switch is one debconf question,**
   `shared/default-x-display-manager`, and each registered package's
   postinst links `display-manager.service` from
   `/etc/X11/default-display-manager` on every configure (`gdm3.postinst`
   lines 26 to 58). greetd's own package does not take part: it registers no
   template and installs its unit unstarted (`debian/rules`:
   `dh_installsystemd --no-stop-on-upgrade --no-start`). `cosmic-greeter`
   takes part with its own unit running `greetd --config …`
   (`debian/cosmic-greeter.service`, `.templates`, `.postinst`).

## Decision

**The order (L1).** Better OS builds the lock screen first (ticket 262),
inside the Better OS session, replacing `cosmic-greeter` as the session's
locker; GDM keeps doing login. The greeter (ticket 264) is built after it,
on the same `login-screen-core`, and is offered as a choice (B) only when:
tickets 262 to 268 have passed on the Ubuntu 24.04 and Zorin OS 18 VMs, the
way back to GDM from a text console has been done by hand in both (ticket
268), and the lock screen has run a week on the owner's machine. The
greeter is never the default while a GNOME session is offered at login (C is
reconsidered only with a new record, when GNOME is no longer a session
Better OS supports or GNOME Shell can lock without GDM).

**What the greeter is (L7).** `greetd` 0.9.0 from Ubuntu's archive, run by
Better OS's own unit `better-os-greeter.service`
(`ExecStart=greetd --config /etc/greetd/better-os-greeter.toml`, otherwise
Debian's `greetd.service`, plus `OnFailure=gdm.service`), with
`cosmic-comp` as the compositor and `better-greeter` (GPUI) as the program,
as the account `_better-greeter`. The package `better-os-greeter` registers
as a Debian display manager exactly as `cosmic-greeter` does (a
`better-os-greeter/daemon_name` template, the shared question's postinst
code), so the choice survives `gdm3` upgrades and `sudo dpkg-reconfigure
gdm3` takes it back.

**Switching on is one action** in Better Manager (ticket 266), carried out by
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
   `better-os-greeter.service`, `systemd-analyze verify` accepts the unit,
   the TOML names `_better-greeter` and an executable command, and the
   account exists; if any fails, put
   back what step 1 captured and report the failure;
5. take effect at the next boot (or when the person presses Restart now);
   the running session is never stopped by the switch.

**Switching off is one action** and puts back exactly what step 1
captured: the debconf value, the file's content, and the link's target;
`dpkg-reconfigure` of the captured package relinks; then the packages step 2
installed are purged (`greetd` only if nothing else depends on it, as the
session's closure may), and `previous.json` is removed. If the captured
display manager is no longer installed, GDM is chosen and the page says so.
Removing the Better OS session switches the login screen off first.

**When it fails on its own.** `OnFailure=gdm.service` starts GDM for the
boot after the fifth failure in 30 s; the login screen watch's greetd form
(ticket 267) also catches a greeter that runs and shows nothing (a stale
heartbeat) and starts GDM after one restart; either way the event is
recorded where ADR 0033's records are and Better Manager reports it, and
the choice stays on until the person changes it.

**The way back by hand** is the numbered steps in
`engineering/specs/login-screen-security.md`, shown next to the switch in
Better Manager.

## Consequences

- Under A nothing about booting changes; the owner sees GDM's login screen
  and Better OS's lock screen until B.
- Under B, a person who switches loses GNOME's lock screen in GNOME sessions
  started from the Better OS greeter, and the ability for a second person to
  log in while one is logged in; the switch's description says both
  (`design/login-screen-spec.md`, section 6).
- The greeter depends on the session's closure (`cosmic-comp`), so it is
  offered only where the Better OS session is installed, and on Ubuntu 24.04
  bases only, as the session is.
- `greetd` 0.9.0 comes from Ubuntu's archive and is already pulled in by
  `cosmic-greeter` (ticket 201's install added it and left it disabled).

## Alternatives not taken

- **C now, or the greeter as the default with the session (S8).** Ticket
  139's failure would be Better OS's to own, on every new install, and GNOME
  would lose its lock screen for everyone still using it.
- **`cosmic-greeter` as the login screen.** ADR 0038's C3 already declined
  it; it is not Better OS's design, and its root daemon reads every
  account's COSMIC configuration from their homes before anyone logs in
  (`daemon/src/main.rs`).
- **Restyling GDM.** GDM's greeter is GNOME Shell; Luma does this by
  patching GNOME Shell, which ADR 0012 does not allow Better OS to do, and
  it would not run in the Better OS session's lock screen.
- **Cage as the compositor.** One more package, no layer shell, one window
  across all outputs (`cage.c` at v0.1.5).
- **Writing `display-manager.service` directly without debconf.** Every
  `gdm3` upgrade would relink it from `/etc/X11/default-display-manager`,
  silently undoing the switch, or, if only the file were changed, linking it
  to Debian's `greetd.service`, which runs `agreety` on VT 7.
