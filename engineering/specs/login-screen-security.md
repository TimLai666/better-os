# The login and lock path: security review

The review Better OS's login and lock path needs before any of it runs on a
person's machine
([ADR 0044](/engineering/decisions/0044-better-os-login-screen-and-gdm.md),
accepted on 2026-11-21, amended on 2026-11-22). It covers six programs: the
greeter (`better-greeter` under greetd), the lock program (`better-lock` in
the Better OS session), the login supervisor
(`better-os-login-supervisor`, which is the display manager), the GNOME
unlock service (`better-os-gnome-unlock`), the backstop and text fallback,
and `better-os-remember-login`. The design is
`design/login-screen-spec.md` (ticket 259). Every claim below names the
source, check or test that holds it, or the ticket that will add the test;
a claim with none is listed under "Not proven". Nothing here has run yet:
the tests named are the ones tickets 260 to 268, 280 and 281 must write.

Revised on 2026-11-22 after an independent review. What changed, in short:
the supervisor is the display manager and no longer `PartOf=` what it
controls; its sandbox no longer breaks Plymouth, and it no longer repairs
accounts at boot; a backstop that runs no Better OS code guards it; the
unlock service's socket follows GDM's (0711 and 0666, checked by peer
credentials); the lock is stated as **not** a boundary against programs
running as the same person; switching and upgrading never stop a running
display manager.

## What it does

- **Login.** At boot, systemd starts `display-manager.service`, which is
  `better-os-login-supervisor.service`. The supervisor checks the graphics
  device, the seat and logind, quits nothing yet, and starts
  `better-os-greeter.service`: `greetd` (root) opens a PAM session of class
  `greeter` for `_better-greeter` and runs `/usr/sbin/better-os-greeter`,
  which execs `cosmic-comp` with `better-greeter`. The greeter takes a
  password, hands it to greetd, and on success asks greetd to start the
  chosen session, then exits. greetd runs PAM (`/etc/pam.d/greetd`) and
  starts the session as the person.
- **Supervising login.** The supervisor watches every stage with a
  deadline, retries the greeter, quits the boot splash, and when the greeter
  cannot appear starts a text login that names the failed stage (spec
  section 5.10). If the supervisor itself cannot run, the backstop does the
  last step.
- **Remembering the choice.** In the new session, `better-os-remember-login`
  (the person) writes the chosen session and language to the person's own
  AccountsService record.
- **Locking the Better OS session.** The session launcher starts
  `better-lock` as the person, inside the session's scope. It takes the
  compositor's session lock (`ext_session_lock_v1`), covers every output,
  and runs PAM itself under `better-os-lock` until the person's password is
  accepted, then unlocks and exits.
- **Locking a GNOME session.** GNOME Shell locks itself; to check the
  password it asks `org.gnome.DisplayManager` for a reauthentication
  channel. While the Better OS login screen is the display manager,
  `better-os-gnome-unlock` (root) owns that name and runs PAM for the
  caller's own account over a private connection (spec section 5.6).

## The risk being reviewed

The login path is the one thing on the machine every person passes through
before being authenticated, and the lock is the only thing between an open
session and whoever is at the keyboard. Two of the programs are new Better
OS code running as root. So the review has to establish, in this order:

1. a password never leaves the path keyboard → program → PAM, is never kept,
   and never reaches a log, a core file, the network, or another account;
2. nothing logs in without PAM saying yes, and the GNOME unlock service
   never checks a password for an account other than the caller's own;
3. a crash or a hang never unlocks a session, never leaves the screen on
   the splash, and never leaves the person without a way to authenticate;
4. the greeter reads nothing a person controls beyond what GDM already
   reads, and nothing at all from a home directory;
5. the root programs can do only what their job needs;
6. the way back to GDM works from a text console with nothing of Better OS
   running.

## Threat model

- **A person at the keyboard of a locked or logged-out machine,** who does
  not know the password. They may type anything, plug in a keyboard or a
  USB device, switch virtual terminals, close and open the lid, unplug a
  monitor, and press keys at any speed. This is who the lock keeps out.
- **Another local account** on the same machine, who can set their own
  avatar, real name and session in AccountsService, and can call
  `org.gnome.DisplayManager.Manager` on the system bus like anyone.
- **A program running as the person** (the person's own, or one that got
  in). It is **not** kept out by the lock, in either session, and this
  review does not claim otherwise: logind lets a session's own user call
  `Unlock` on it with no password (systemd v255
  `src/login/logind-session-dbus.c` lines 201 to 221: the polkit check
  names the session's uid as the user who needs no authorization), and
  both Better OS's lock and GNOME's honour `Unlock`; in the Better OS
  session such a program can also kill `better-lock` and take the
  compositor's lock in the second before the launcher restarts it, since
  `cosmic-comp` accepts a lock from any client once the old one is gone
  (`session_lock.rs` lines 22 to 30), and then unlock it. What such a
  program can do while the person is away it could already do while they
  were there. Closing the compositor path would need a `cosmic-comp` patch
  (ADR 0039); it is listed, not proposed.
- Out of scope: root, physical access to the disk, the kernel, firmware.

## Who holds which privilege

| Process | Account | Started by | Holds |
| --- | --- | --- | --- |
| `better-os-login-supervisor` | root | `display-manager.service` (its alias), `Type=notify`, `WatchdogSec=10`, `Restart=always`, `StartLimitBurst=5` in 60 s, `OnFailure=better-os-login-backstop.service` | start and stop of exactly `better-os-greeter.service`, `better-os-gnome-unlock.service`, `better-os-login-fallback@tty1.service` and `@tty2.service` through systemd's bus; `plymouth` commands and a kill of `plymouthd`; `KDSETMODE` and `VT_SETMODE` on `/dev/tty1`, `chvt`; reads logind, `/sys/class/drm`, the greeter's heartbeat, greetd's cgroup; writes `/var/lib/better-os/boot-watch/` and `/run/better-os/` |
| `better-os-login-backstop` (a unit, no program of Better OS's) | root | the supervisor's `OnFailure=` and a 90 s timer | `plymouth quit`, `systemctl start better-os-login-fallback@tty2.service`, `chvt 2`, only when the greeter never reported `ready` and no fall back runs |
| `greetd` 0.9.0 | root | `better-os-greeter.service`, started only by the supervisor | PAM for `greetd` and `greetd-greeter`, the VT, the socket it gives to the greeter (`greetd/src/server.rs` line 182) |
| `better-os-gnome-unlock` | root | `better-os-gnome-unlock.service` (`Type=dbus`, `Conflicts=gdm.service`) | the bus name `org.gnome.DisplayManager` (under its own bus policy file); PAM for the services `better-os-gnome-unlock`, `-fingerprint`, `-smartcard`; `/run/better-os-gnome-unlock/` |
| `/usr/sbin/better-os-greeter`, `cosmic-comp`, `better-greeter` | `_better-greeter` | greetd's greeter session | the seat's devices through logind (class `greeter`); greetd's socket; the system bus as an unprivileged client; `/var/lib/better-os-greeter/` and `/run/better-os-greeter/` |
| Orca, AT-SPI bus (only with the screen reader on) | `_better-greeter` | the start wrapper | the greeter's own D-Bus session |
| `agetty` and `login` (fall back) | root, then the person | `better-os-login-fallback@ttyN.service` | the standard text login, PAM service `login` |
| `better-os-remember-login` | the person | an XDG autostart entry | AccountsService for the person's own account (`change-own-user-data`) |
| `better-lock` | the person | the session launcher | the person's PAM conversation under `better-os-lock`; the compositor's session lock; the session bus |
| `cosmic-greeter` locker (fallback only) | the person | the session launcher after three crashes | the same, under its own PAM service |

`_better-greeter` is a system account created by sysusers at install: no
password, no shell (`/usr/sbin/nologin`), home `/var/lib/better-os-greeter`
(0700), member of `video` only if `cosmic-comp` needs it there (ticket 268).
The greeter never writes AccountsService for anyone: changing another
account's record needs `org.freedesktop.accounts.user-administration`, which
polkit grants only with an administrator's password (`auth_admin_keep`,
`/usr/share/polkit-1/actions/org.freedesktop.accounts.policy`).

## What each can reach

`better-greeter`, after its sandbox is applied (ticket 263; applied in
`main` before the first D-Bus call or file read):

| Resource | Allowed | How it is held |
| --- | --- | --- |
| greetd's socket | connect, read, write | `GREETD_SOCK`; the only Unix stream socket it opens besides the system bus and the Wayland socket |
| System bus | method calls and property reads on AccountsService (reads only), logind (`Can*`, `Suspend`, `Reboot`, `PowerOff`, `ListSessions`), NetworkManager and UPower (properties only), locale1, and `org.gnome.DisplayManager`'s `Version` property | the bus's own policy; logind's power calls are decided by polkit for an active greeter session; a test lists every destination in the code |
| Files read | `/usr/share/` (artwork, fonts, `wayland-sessions`), `/etc/fonts`, `/var/lib/AccountsService/icons/`, its own state directories, `$XDG_RUNTIME_DIR` | Landlock (ABI 4, Linux 6.7 and newer; Ubuntu 24.04 ships 6.8) |
| Files written | its state directories and `$XDG_RUNTIME_DIR` | Landlock; files created new with `O_NOFOLLOW` and renamed into place |
| Network | none | Landlock's TCP bind and connect rules, and a seccomp filter refusing `socket(AF_INET, …)`, `AF_INET6`, `AF_PACKET`; a test runs the built binary and checks that a `connect` attempt from inside fails |
| Other processes | none | seccomp refuses `ptrace`, `process_vm_readv`, `process_vm_writev`; `PR_SET_DUMPABLE` is 0 |
| Executing programs | none after start | seccomp refuses `execve` once the screen is up; Orca is started by the wrapper before |

`better-lock` runs with the person's rights. It sets `PR_SET_DUMPABLE` 0 at
start, so another process of the person cannot read its memory with
`ptrace` without `CAP_SYS_PTRACE`; that protects the password it is holding,
not the lock (above). It uses the same no-network and no-`execve` rules.

### The supervisor

It is root because it must be able to stop a display manager and start a
text login when every other program has failed. What limits it:

- It runs no PAM, reads no password, and has no D-Bus interface that changes
  anything (`org.betteros.LoginSupervisor1` offers only the last record,
  read-only, for `better-os-login-status`).
- The units it acts on are a fixed list compiled in (above); a test feeds it
  every stage outcome against a fake systemd and checks it never names
  another unit. It is not `PartOf=`, `BindsTo=` or `Requires=` any of them,
  so stopping them never stops it (systemd.unit(5), `PartOf=`: "When systemd
  stops or restarts the units listed here, the action is propagated to this
  unit").
- Its unit sandbox: `ProtectHome=yes`; `ProtectSystem=strict` with
  `ReadWritePaths=/var/lib/better-os/boot-watch /run/better-os`; the network
  closed by `IPAddressDeny=any`, **not** by `PrivateNetwork=`, because a
  private network namespace also hides abstract Unix sockets
  (network_namespaces(7)) and `plymouth` reaches `plymouthd` only at the
  abstract address `/org/freedesktop/plymouthd` (in `libply-boot-client.so.5`);
  `RestrictAddressFamilies=AF_UNIX` (it polls `/sys/class/drm` and does not
  open a udev monitor, so it needs no `AF_NETLINK`); `NoNewPrivileges=yes`;
  `MemoryDenyWriteExecute=yes`; `SystemCallFilter=@system-service` plus the
  `ioctl`s on `/dev/tty1` it needs (`DeviceAllow=/dev/tty1 rw`,
  `/dev/tty2 rw`).
- It does not repair accounts. `/etc` stays read-only to it; a missing or
  locked `_better-greeter` is a failed stage 5 and a text login, and Better
  Manager's health check repairs it from the person's next session.
- What it writes to the fallback's issue file is its own fixed text with a
  stage name from a closed list and a time.
- Its own state lives in `/run/better-os/login-path.json`, so a watchdog
  restart continues the same appearance instead of starting its deadlines
  again.

### The backstop

`better-os-login-backstop.service` runs only `plymouth`, `systemctl` and
`chvt`, with `ExecCondition=` on two files in `/run`; it exists so that a
bug in the supervisor (a crash loop, a binary that will not start, a sandbox
that refuses it) still ends in a text login. It is started by the
supervisor's `OnFailure=` (after five starts in 60 s) and by a timer 90
seconds after the supervisor's first start.

### The GNOME unlock service

It is root because `pam_unix` checks a password for another uid only as
root, and because only root may own `org.gnome.DisplayManager`. It ships its
own bus policy file with the same rules as `gdm3`'s `gdm.conf` (root may
own the name; everyone may call `org.gnome.DisplayManager.Manager` and read
its properties), so it works on an Ubuntu machine where `gdm3` has been
removed. What limits it:

- **Who may ask.** `OpenReauthenticationChannel(username)` is answered only
  when, from the bus daemon's `GetConnectionCredentials` (never from
  anything the caller says), the caller's process is in a logind session
  that is local, on a seat, of class `user`, and owned by `username`'s uid.
  Every other caller gets `AccessDenied`. This is the boundary: **a caller
  can only have its own account's password checked.** The first version
  also compared the caller's executable with `/usr/bin/gnome-shell`; that
  is kept as a filter against mistakes but is not a boundary, because the
  same person can run their own `/usr/bin/gnome-shell` (with `LD_PRELOAD`,
  a shell mode, or an extension). GDM itself checks neither the executable
  nor that `username` is the caller (`gdm-manager.c` lines 1155 to 1235).
- **The private connection.** As GDM does it (`daemon/gdm-dbus-util.c`
  lines 84 to 107): a directory `/run/better-os-gnome-unlock/` at 0711 and
  a socket in it at 0666 for each accepted request, so the person's
  `gnome-shell` can connect; the boundary is the connection's credentials,
  not the file's mode. The service accepts exactly one connection within 10
  seconds whose peer credentials (`SO_PEERCRED`, and the uid D-Bus's
  `EXTERNAL` authentication reports) match the requesting caller's uid and
  pid, then removes the socket. The first version put the socket in a 0700
  root directory, which the person's `gnome-shell` could not reach: every
  unlock would have failed with "Authentication error".
- **What it runs.** PAM for that one account, only for the three service
  names it maps (`gdm-password` → `better-os-gnome-unlock`,
  `gdm-fingerprint` → `-fingerprint`, `gdm-smartcard` → `-smartcard`, Better
  OS's own files): `pam_authenticate`, `pam_acct_mgmt`, then `pam_setcred`
  with `PAM_REINITIALIZE_CRED`, with the session's `XDG_RUNTIME_DIR` and
  session bus address from logind in PAM's environment, as GDM's worker
  has. It opens no PAM session and starts no process.
- **Limits it sets itself.** Each conversation runs on its own thread with a
  60-second limit (a hung `pam_sss` stalls only that conversation, and is
  cancelled); at most one channel per session and one conversation per
  service name; after five failures for a uid, 1, 2, 4, 8, then 30 seconds
  between attempts across all service names. `/etc/pam.d` on the owner's
  machine has no `pam_faillock`, so without this only `pam_faildelay`'s 3
  seconds would apply.
- **Its unit sandbox.** `ProtectHome=read-only`; the network closed with
  `IPAddressDeny=any` unless a PAM module in `common-auth` needs it
  (`pam_sss` does on machines that use it; decided per machine at install
  and recorded, **not proven**); `NoNewPrivileges=yes`;
  `RestrictAddressFamilies=AF_UNIX AF_NETLINK`.
- **Never beside GDM, and never stopped under a locked session.**
  `Conflicts=gdm.service`; switching back to GDM takes effect at the next
  boot, so it is never stopped while a GNOME session that needs it runs. An
  upgrade `try-restart`s it; a GNOME unlock attempt in that second fails
  with "Authentication error" and the next one works. A new `libgdm1` major
  version never makes it refuse sessions that are already running.

## The password

- **Where it lives.** One fixed-capacity buffer per prompt (256 bytes, as
  Better OS's field stops at 256 characters), allocated once, locked in
  memory with `mlock`, owned by `login-screen-core`'s `Secret` type, which
  implements no `Debug`, `Display`, `Clone` or `Serialize`, and wipes itself
  with `zeroize` on drop. The GNOME unlock service uses the same type for
  the answers the shell sends. The buffer is its own page-aligned
  allocation, so its `munlock` never unlocks a page another value shares,
  and is marked `MADV_DONTDUMP`. Held by ticket 260's tests: the bytes are
  zero after `wipe()` and after a Backspace (`secret.rs`), no freed block
  ever holds a password, for a `Secret` and for the greetd message built
  from it (`tests/secret_wiped_on_drop.rs`, a watching global allocator,
  with a positive control and checked to fail when the wipe on drop is
  removed), and none of `Debug`, `Display`, `Clone` or `Serialize` is
  implemented by either type. The view model (`login_screen_core::view`)
  never holds a password: it only says when the field's `Secret` is sent
  and when it is wiped. The drop test reads only page-aligned blocks, the
  password buffers' own, which are allocated zeroed, so it never reads a
  byte nothing wrote.
- **When the lock fails.** `mlock` can fail (an `RLIMIT_MEMLOCK` of 0, a
  container's limit). The buffer is then not locked and may be written to
  swap; it still wipes itself. This is not silent: `Secret::is_locked` and
  `Secret::lock_warning` say so, and the greeter and the lock program write
  `login_screen_core::NOT_LOCKED_LOG_LINE` (`BETTER_OS_SECRET_LOCKED=no`, the
  cause, no content) to the journal once (tickets 264 and 262). The test
  that the pages are locked needs an `RLIMIT_MEMLOCK` of at least 1 MiB,
  through `test_prerequisites::require_that!` (a skip on a developer's
  machine, a failure under CI). A page size the kernel does not report is
  an error (`SecretError::PageSize`), never a guessed 4096, which on a
  16 KiB or 64 KiB page arm64 kernel would break the page alignment.
- **Who may read it.** Only `Secret::expose_str` gives the text out, and
  only the greetd message builder calls it; `tests/one_secret_reader.rs`
  reads every crate's source and fails on any other caller. Ticket 262 adds
  its PAM conversation to that test's audited list in the change that adds
  it.
- **Where it goes.** In the greeter, into greetd's
  `post_auth_message_response` JSON, built directly into a second `Secret`
  buffer, written to the socket, then both are wiped. In the lock program
  and the unlock service, into PAM's conversation reply, which PAM frees
  itself; the program's copy is wiped when the reply is handed over.
- **When it is wiped.** After it is sent; on Escape; when the view changes;
  after 60 seconds without input; when PAM's question turns out to be one
  whose answer is shown (`visible`), so nothing typed hidden or queued for a
  secret question appears in clear; when an answer queued for a question
  that never came is abandoned; when the machine is about to sleep, however
  that was asked for, and when it wakes; when the program exits normally.
  Each is a test in `login_screen_core::view`.
- **Late replies.** Every conversation has an id; a reply from one that was
  cancelled (a switch of account, two Escapes, a sleep) is dropped in
  `login_screen_core::view`, so it can neither count a failure against the
  next account, show its question in the next one's field, nor unlock.
  Tested with a cancel, a new begin, and every kind of stale reply.
- **What is never done with it.** No log line, no error message, no panic
  message carries it; a test feeds a known secret through each path with
  logging at its most verbose and searches the log and every error value
  for it (tickets 263 and 280).
- **Input methods.** The greeter runs none. The lock surface takes the
  keyboard; the field asks for the `password` content purpose and the lock
  program keeps text input disabled.

## What unlocks and what logs in

- The greeter cannot start a session without greetd's `success` for that
  account (`context.rs`, `start`).
- `better-lock` calls `unlock_and_destroy` only after PAM's `authenticate`
  and `acct_mgmt` both succeed for the session's own user, or when logind
  sends the session `Unlock`. logind sends `Unlock` to anyone the session's
  user or an administrator lets call it; for the person this needs no
  password (threat model). A test holds that every other outcome leaves the
  lock held.
- GNOME Shell unlocks on `VerificationComplete` from the channel or on
  logind's `Unlock` (`screenShield.js` lines 147 to 156); the unlock
  service sends `VerificationComplete` only after the three PAM calls above
  succeed for the caller's own account.
- Nothing draws over a Better OS lock but the lock surfaces and layers
  marked to show on the lock (`src/shell/focus/order.rs` lines 91 to 177);
  any program of the person's may mark one, so it could imitate the lock
  screen; it could also simply unlock (threat model).

## What a failure leaves the person with

| What fails | What the person sees | What brings it back |
| --- | --- | --- |
| Any stage of the spec's section 5.10 | the splash or a black screen for at most 80 seconds | the supervisor retries, then starts the text login naming the stage; never the splash, never GDM on its own |
| A process of the greeter path that the kernel cannot kill | the fall back starts on tty2 without waiting for it | whether the text console can draw while that process holds the device depends on the driver: **not promised** |
| The greeter hangs after it was ready | a frozen login screen | its heartbeat goes stale; the supervisor restarts it within 30 s |
| The supervisor hangs | nothing at first | its watchdog aborts it within 10 s and `Restart=always` starts it again; it continues from `/run/better-os/login-path.json` |
| The supervisor cannot start or keeps crashing | nothing from it | after five starts in 60 s, `OnFailure=` starts the backstop; in any case the backstop's timer fires at 90 s: `plymouth quit` and a text login on tty2 |
| logind crashes at logout (ticket 256) | a black screen | the supervisor sees the session's end in greetd's cgroup as well as in logind, and starts a new appearance |
| The GNOME unlock service crashes | a GNOME lock screen saying "Authentication error" | systemd restarts it within a second; the next attempt opens a new channel |
| The GNOME unlock service is absent when a GNOME session starts | — | the greeter checks it right before starting GNOME and does not start GNOME without it (spec section 5.6) |
| `better-lock` crashes | every output black (the lock stays) | the launcher starts it again within a second; after three crashes in 30 s, `cosmic-greeter`'s locker; if both fail, the steps below |
| `better-lock` hangs | a frozen lock screen | its heartbeat; the launcher restarts it after 30 s of silence |
| The session launcher dies while locked | the compositor's session ends and the login screen appears | logging in again; unsaved work in that session is lost |

ADR 0033's watch is not used on this path: it does nothing when the display
manager is not GDM. The supervisor replaces it here and writes its records
in the same place and format.

## The way back to GDM from a text console

For a person who wants GDM back, or whose machine fell back to the text
login. Every step works with nothing of Better OS running.

1. If the screen shows the Better OS text login, go to step 2. Otherwise
   press **Ctrl+Alt+F3**; if the screen stays black, try **Ctrl+Alt+F4** to
   **F6**.
2. At `login:`, type the user name of an account that can use `sudo` (the
   first account made on the machine), press Enter, then its password and
   Enter. Nothing is shown while the password is typed.
3. Type `sudo dpkg-reconfigure gdm3` and press Enter, then the password
   again.
4. A list titled "Default display manager" appears. Move to **gdm3** with
   the arrow keys and press **Enter**. (If no list appears, GDM was already
   chosen: go to step 6.)
5. Type `cat /etc/X11/default-display-manager` and Enter. It must print
   `/usr/sbin/gdm3`. Type `ls -l /etc/systemd/system/display-manager.service`
   and Enter; it must end in `gdm3.service`.
6. If either is wrong, type these three lines, each followed by Enter:
   `echo /usr/sbin/gdm3 | sudo tee /etc/X11/default-display-manager`
   `sudo ln -sf /lib/systemd/system/gdm3.service /etc/systemd/system/display-manager.service`
   `sudo systemctl daemon-reload`
7. Type `sudo reboot` and Enter. GDM's login screen appears after the
   restart. (Restarting `display-manager.service` instead would end every
   running session, including GNOME sessions whose lock needs the unlock
   service.)
8. Once logged in, open Better Manager: its Better OS session page shows
   "Use the Better OS login screen" off, and finishes removing it.

Ticket 268 performs these steps by hand in both VMs and records the
screens; Better Manager shows the same text next to the switch.

## Getting back into a locked session

For a session whose lock screen stays black, frozen, or cannot check a
password.

1. Press **Ctrl+Alt+F3** (or F4 to F6).
2. Log in with **the same account** as the locked session, with its
   password.
3. Type `loginctl list-sessions` and Enter, and note the id on the line with
   your user name and `seat0`.
4. Type `loginctl unlock-session <id>` with that id, and Enter. logind
   allows it for your own session without another password; both Better
   OS's lock screen and GNOME's honour it.
5. Switch back with **Ctrl+Alt+F1** (or the terminal `loginctl` lists for
   the session).
6. If nothing brings the session back, `loginctl terminate-session <id>`
   ends it: **everything not saved in it is lost**, and the login screen
   appears.

In the Better OS session, `better-os-session relock` (ticket 262) starts the
lock program again instead of unlocking, for a person who wants the lock
screen back rather than the session unlocked.

## What an attacker gains

- **At the keyboard:** a password prompt backed by PAM's own rules and
  delay, with the greeter's doubling wait on top; the text fallback is the
  standard `login`. The doubling wait is kept per user name and is not
  reset by anything the person at the keyboard can do: Switch account is
  not offered while it runs, choosing the account again (or typing its name
  under "Other account…") finds it still running, and a restart of the
  greeter or the lock program reads it back from the program's own record
  (`/run/better-os-greeter/attempts.json`, written only by
  `_better-greeter`, mode 0600 in a 0700 directory; the lock program's in the
  person's `$XDG_RUNTIME_DIR/better-os/`). Only a reboot, which empties
  `/run`, ends it early; spec section 3.9. Text another program puts on the
  lock screen (notifications, media) is put on one line, stripped of control
  and direction characters and capped (spec section 4.2), so it cannot
  imitate a system message there. Nothing to open, no launcher, no network menu. The power
  menu can restart the machine, as the power button can. Switching virtual
  terminals shows another login prompt or the locked session's lock, never
  its content (the lock is in the session's compositor in both sessions).
- **Another local account:** can put a crafted avatar or name in front of
  the greeter (decoded by the pure-Rust `image` crate with size limits,
  inside the sandbox); can ask the unlock service only about its own
  account, from its own local session, under the service's own backoff. A
  code execution in the greeter gains `_better-greeter`: its state
  directories and greetd's socket (trying passwords as anyone at the
  keyboard can, under PAM's delay), nothing else.
- **A program running as the person:** can unlock the person's session
  without the password (threat model). It cannot read the lock program's
  memory, and it gains nothing about other accounts.
- **A compromise of the supervisor or the unlock service** is root. That
  is why each has a fixed list of what it may do, its own unit sandbox, no
  input it did not create beyond its D-Bus methods, and tests that hold
  each limit.

## Not proven

- Only the password buffer's tests exist (ticket 260); the others named
  above are for tickets 261 to 268, 280 and 281.
- That GNOME Shell 46 locks and unlocks through the unlock service as it
  does through GDM, that GDM's session worker does nothing more on
  reauthentication that the shell relies on, how `pam_gnome_keyring`
  behaves when PAM runs in a root service, and what GNOME's "Switch user"
  button does there (ticket 280).
- Whether the unlock service can close the network on machines whose
  `common-auth` names a networked PAM module.
- Whether `cosmic-comp` shows any surface but the lock's while locked, in
  practice (ticket 268 tries a layer surface over a lock).
- Whether `_better-greeter` needs the `video` group under `cosmic-comp`.
- Whether Landlock's TCP rules and the seccomp filter leave GPUI, Orca's
  AT-SPI bus and AccessKit working (ticket 263).
- Every deadline of the supervisor and the backstop, the `KD_TEXT` reset of
  tty1, and greetd's cgroup as the sign of a session's end (ticket 281).
