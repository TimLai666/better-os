# The login and lock path: security review

The review Better OS's login and lock path needs before any of it runs on a
person's machine, and before the Better OS greeter may replace GDM
([ADR 0044](/engineering/decisions/0044-better-os-login-screen-and-gdm.md),
proposed). It covers five programs: the greeter (`better-greeter` under
greetd), the lock program (`better-lock` in the Better OS session), the
login supervisor (`better-os-login-supervisor`), the GNOME unlock service
(`better-os-gnome-unlock`), and the text fallback. The design is
`design/login-screen-spec.md` (ticket 259). Every claim below names the
source, check or test that holds it, or the ticket that will add the test;
a claim with none is listed under "Not proven". Nothing here has run yet:
the tests named are the ones tickets 260 to 268, 280 and 281 must write.

## What it does

- **Login.** At boot and after every logout, `greetd` (root) opens a PAM
  session of class `greeter` for the account `_better-greeter` and runs
  `/usr/sbin/better-os-greeter`, which execs `cosmic-comp` with
  `better-greeter` as its program. The greeter shows the accounts
  AccountsService lists, takes a password, hands it to greetd over greetd's
  socket, and on success asks greetd to start the chosen session, then
  exits. greetd runs PAM (`/etc/pam.d/greetd`) and starts the session as
  the person.
- **Supervising login.** `better-os-login-supervisor` (root) watches every
  stage between the display manager's start and a login prompt accepting
  input, with a deadline for each; it retries the greeter, quits the boot
  splash, and when the greeter cannot appear starts a text login on the
  first virtual terminal (spec section 5.10).
- **Locking the Better OS session.** The session launcher starts
  `better-lock` as the person, inside the session's scope. It takes the
  compositor's session lock (`ext_session_lock_v1`), covers every output,
  and runs PAM itself under the service `better-os-lock` until the
  person's password is accepted, then unlocks and exits.
- **Locking a GNOME session.** GNOME Shell locks itself; to check the
  password it asks `org.gnome.DisplayManager` for a reauthentication
  channel. While the Better OS greeter is the display manager,
  `better-os-gnome-unlock` (root) owns that name and runs PAM for the
  caller's own user over a private connection (spec section 5.6).

## The risk being reviewed

The login path is the one thing on the machine every person passes through
before being authenticated, and the lock is the only thing between an open
session and whoever is at the keyboard. Two of the five programs are new
Better OS code running as root. So the review has to establish, in this
order:

1. a password never leaves the path keyboard → program → PAM, is never kept,
   and never reaches a log, a core file, the network, or another account;
2. nothing unlocks or logs in without PAM saying yes, and the GNOME unlock
   service never checks a password for anyone but the session that asks;
3. a crash or a hang of any of them never leaves a session unlocked, never
   leaves the screen on the splash, and never leaves the person without a
   way to authenticate;
4. the greeter reads nothing a person controls beyond what GDM already
   reads, and nothing at all from a home directory;
5. the root programs can do only what their job needs;
6. the way back to GDM works from a text console with nothing of Better OS
   running.

## Threat model

- **A person at the keyboard of a locked or logged-out machine,** who does
  not know the password. They may type anything, plug in a keyboard or a
  USB device, switch virtual terminals, close and open the lid, unplug a
  monitor, and press keys at any speed.
- **Another local account** on the same machine, who can set their own
  avatar, real name and session in AccountsService, and can call
  `org.gnome.DisplayManager.Manager` on the system bus like anyone
  (`gdm.conf`'s default policy).
- **A program in the person's session** (the person's own, or one that got
  in), which can talk to the session and system buses and the compositor but
  does not know the password. It may try to draw over the lock screen, to
  read the lock program's memory, to ask it to unlock, or to open a
  reauthentication channel itself.
- Out of scope: root, physical access to the disk, the kernel, firmware.

## Who holds which privilege

| Process | Account | Started by | Holds |
| --- | --- | --- | --- |
| `greetd` 0.9.0 | root | `better-os-greeter.service` | PAM for both its services, the VT, the socket it gives to the greeter (`greetd/src/server.rs` line 182 `chown`s it to the greeter's account) |
| `better-os-login-supervisor` | root | `better-os-login-supervisor.service` (`WatchdogSec=10`) | start, stop and restart of exactly `better-os-greeter.service`, `better-os-login-fallback@tty1.service` and `plymouth-quit.service`, through systemd's bus; `plymouth` commands and a kill of `plymouthd`; `chvt 1`; `systemd-sysusers` for its own account file; reads logind, `/sys/class/drm`, the greeter's heartbeat; writes `/var/lib/better-os/boot-watch/` and `/run/better-os/login-fallback.issue` |
| `better-os-gnome-unlock` | root | `better-os-gnome-unlock.service` (`Conflicts=gdm.service`) | the bus name `org.gnome.DisplayManager`; PAM for the services `better-os-gnome-unlock`, `-fingerprint`, `-smartcard`; one socket directory `/run/better-os-gnome-unlock/` (0700, root) |
| `/usr/sbin/better-os-greeter` | `_better-greeter` | greetd's greeter session | nothing; sets the environment and execs the compositor |
| `cosmic-comp` (greeter) | `_better-greeter` | the line above | the seat's DRM and input devices through logind (class `greeter` session, active on `seat0`) |
| `better-greeter` | `_better-greeter` | `cosmic-comp` | greetd's socket; the system bus as an unprivileged client; read and write in `/var/lib/better-os-greeter/` and `/run/better-os-greeter/` only |
| Orca, AT-SPI bus (only with the screen reader on) | `_better-greeter` | the start wrapper | the greeter's own D-Bus session; reads the greeter's accessibility tree |
| `agetty` and `login` (fallback) | root, then the person | `better-os-login-fallback@tty1.service` | the standard text login, PAM service `login` |
| `better-lock` | the person | the session launcher | the person's PAM conversation under `better-os-lock`; the compositor's session lock; the session bus |
| `cosmic-greeter` locker (fallback only) | the person | the session launcher after three crashes | the same, under its own PAM service |

`_better-greeter` is a system account created by sysusers: no password, no
shell (`/usr/sbin/nologin`), home `/var/lib/better-os-greeter` (0700),
member of `video` only if `cosmic-comp` needs it there (ticket 268). It owns
no file outside its home and `/run/better-os-greeter/`.

## What each can reach

`better-greeter`, after its sandbox is applied (ticket 263; applied in
`main` before the first D-Bus call or file read):

| Resource | Allowed | How it is held |
| --- | --- | --- |
| greetd's socket | connect, read, write | `GREETD_SOCK`; the only Unix stream socket it opens besides the system bus and the Wayland socket |
| System bus | method calls and property reads on AccountsService, logind (`Can*`, `Suspend`, `Reboot`, `PowerOff`, `ListSessions`), NetworkManager and UPower (properties only), locale1, and `org.gnome.DisplayManager`'s `Version` property (to know whether to list GNOME) | the bus's own policy; logind's power calls are decided by polkit for an active greeter session; a test lists every destination in the code |
| Files read | `/usr/share/` (artwork, fonts, `wayland-sessions`), `/etc/fonts`, `/var/lib/AccountsService/icons/`, its own state directories, `$XDG_RUNTIME_DIR` | Landlock (ABI 4, Linux 6.7 and newer; Ubuntu 24.04 ships 6.8) |
| Files written | its state directories and `$XDG_RUNTIME_DIR` | Landlock; files created new with `O_NOFOLLOW` and renamed into place |
| Network | none | Landlock's TCP bind and connect rules, and a seccomp filter refusing `socket(AF_INET, …)`, `AF_INET6`, `AF_PACKET`; a test runs the built binary and checks that a `connect` attempt from inside fails |
| Other processes | none | seccomp refuses `ptrace`, `process_vm_readv`, `process_vm_writev`; `PR_SET_DUMPABLE` is 0 |
| Executing programs | none after start | seccomp refuses `execve` once the screen is up; Orca is started by the wrapper before |

`better-lock` runs with the person's rights. It sets `PR_SET_DUMPABLE` 0 at
start (so `ptrace` from the same account is refused without
`CAP_SYS_PTRACE`) and uses the same no-network and no-`execve` rules.

`better-os-login-supervisor` is root and needs to be: it must be able to
stop a display manager and start a text login when every other program has
failed. What limits it:

- It runs no PAM, reads no password, opens no socket but the system bus and
  the journal, and has no D-Bus interface of its own that changes anything
  (`org.betteros.LoginSupervisor1` offers only the last record, read-only,
  for `better-os-login-status`).
- The units it acts on are a fixed list compiled in (above); a test feeds it
  every stage outcome against a fake systemd and checks it never names
  another unit.
- Its unit sandbox: `ProtectHome=yes`, `ProtectSystem=strict` with
  `ReadWritePaths=/var/lib/better-os/boot-watch /run/better-os`,
  `PrivateNetwork=yes`, `NoNewPrivileges=yes`, `RestrictAddressFamilies=AF_UNIX`,
  `MemoryDenyWriteExecute=yes`, `SystemCallFilter=@system-service`. It reads
  `/sys/class/drm` and never writes it.
- What it writes to the fallback issue file is its own fixed text with a
  stage name from a closed list and a time; nothing from another program.

`better-os-gnome-unlock` is root because PAM's `pam_unix` checks a
password through `unix_chkpwd` for its own user only unless it runs as root,
and because only root may own `org.gnome.DisplayManager` (`gdm.conf`). What
limits it:

- **Who may ask.** `OpenReauthenticationChannel(username)` is answered only
  when, from the caller's bus credentials (the bus daemon's
  `GetConnectionCredentials`, not anything the caller says): the caller's
  process is in a logind session that is local, on a seat, and of class
  `user`; that session's user is `username`; and the caller's executable
  (`/proc/<pid>/exe`, opened once, compared by device and inode with
  `/usr/bin/gnome-shell`) is GNOME Shell. Every other caller gets
  `AccessDenied`. Another local account therefore cannot have it check a
  password for anyone, not even itself unless it is running GNOME Shell.
- **The private connection.** For each accepted request it creates one
  socket under `/run/better-os-gnome-unlock/` (0700, root), returns its
  address, accepts exactly one connection within 10 seconds whose peer
  (`SO_PEERCRED`) has the caller's uid and pid, and closes the socket.
  Nothing else can connect: the directory is root's, and the one connection
  is matched against the request.
- **What it runs.** PAM for that one user, only for the three service names
  it maps (`gdm-password` → `better-os-gnome-unlock`, `gdm-fingerprint` →
  `-fingerprint`, `gdm-smartcard` → `-smartcard`, Better OS's own files in
  `/etc/pam.d/`): `pam_authenticate`, `pam_acct_mgmt`, then `pam_setcred`
  with `PAM_REINITIALIZE_CRED`. It opens no PAM session and starts no
  process. Answers from the shell are kept in the same `Secret` buffer type
  as the greeter's (below).
- **Limits.** At most one channel per session at a time, at most one
  conversation per service per channel; a channel idle for 5 minutes is
  closed. Failed attempts are subject to PAM's own delay (`pam_faildelay`
  in the file, 3 seconds as in `/etc/pam.d/login`).
- **Its unit sandbox.** `ProtectHome=read-only` (PAM modules may read
  `~/.pam_environment`-style files; none in the Better OS files do, which a
  test checks), `PrivateNetwork=yes` unless a PAM module in its files needs
  the network (`pam_sss` does; Better OS's files include `common-auth`,
  which may name it, so this is decided per machine and recorded: **not
  proven**), `NoNewPrivileges=yes`, `RestrictAddressFamilies=AF_UNIX`,
  `CapabilityBoundingSet=` only what PAM needs.
- **Never beside GDM.** `Conflicts=gdm.service`; started only while the
  alias names the Better OS greeter. If GDM owns the name, it never starts.

## The password

- **Where it lives.** One fixed-capacity buffer per prompt (256 bytes, as
  Better OS's field stops at 256 characters), allocated once, locked in
  memory with `mlock`, owned by `login-screen-core`'s `Secret` type, which
  implements no `Debug`, `Display`, `Clone` or `Serialize`, and wipes itself
  with `zeroize` on drop. The GNOME unlock service uses the same type for
  the answers the shell sends.
- **Where it goes.** In the greeter, into greetd's
  `post_auth_message_response` JSON, built directly into a second `Secret`
  buffer (never a `String` or a `serde_json::Value`), written to the socket,
  then both are wiped. In the lock program and the unlock service, into
  PAM's conversation reply, which PAM frees itself; the program's copy is
  wiped when the reply is handed over.
- **When it is wiped.** After it is sent; on Escape; when the view changes;
  after 60 seconds without input; when the program exits normally. The
  field's drawn text is dots only, except while "Show password" is held on.
- **What is never done with it.** No log line, no error message, no panic
  message carries it; a test feeds a known secret through each path with
  logging at its most verbose and searches the log and every error value
  for it (tickets 263 and 280).
- **Input methods.** The greeter runs none. The lock surface takes the
  keyboard from the session; the field asks for the `password` content
  purpose and the lock program keeps text input disabled.

## Nothing unlocks without PAM, or without logind

- The greeter cannot start a session without greetd's `success` for that
  account: greetd refuses `start_session` unless the session is ready
  (`context.rs`, `start`).
- `better-lock` calls `unlock_and_destroy` only after PAM's `authenticate`
  and `acct_mgmt` both succeed for the session's own user, or when logind
  sends the session `Unlock`. logind sends `Unlock` only to a caller it has
  authorized for that session (`loginctl unlock-session`); that is the
  text-console way back, after the person has authenticated there. The rule
  is in `login-screen-core` with a test that every other outcome (error,
  conversation failure, PAM module missing, a panic in the conversation)
  leaves the lock held.
- GNOME Shell unlocks only on `VerificationComplete` from the channel or on
  logind's `Unlock` (`screenShield.js` lines 147 to 156); the unlock
  service sends `VerificationComplete` only after the three PAM calls above
  succeed for the caller's own user.
- Nothing draws over a Better OS lock but the lock surfaces and layers
  marked to show on the lock: `cosmic-comp` draws nothing else while locked
  (`src/shell/focus/order.rs` lines 91 to 177). The marking
  (`cosmic_session_lock_layer_manager_v1`) is offered to every client that
  is not sandboxed (`src/state.rs` line 776), so a program of the person's
  could imitate the lock screen above it; it cannot unlock. Listed under
  "Not proven" until ticket 268 checks it.

## What a failure leaves the person with

| What fails | What the person sees | What brings it back |
| --- | --- | --- |
| The greeter, its compositor, greetd, the greeter's account, logind, the DRM device, Plymouth (every stage of the spec's section 5.10) | the splash or a black screen for at most 60 seconds | the supervisor retries, then starts the text login with a message naming the stage; never the splash, never GDM on its own |
| The greeter hangs after it was ready | a frozen login screen | its heartbeat goes stale; the supervisor restarts it within 30 s, and the stages run again |
| The supervisor hangs or crashes | nothing at first | systemd restarts it on its watchdog (`WatchdogSec=10`); its deadlines start again from its own restart |
| The GNOME unlock service crashes | a GNOME lock screen saying "Authentication error" | systemd restarts it within a second; the next password attempt opens a new channel; the session stays locked meanwhile |
| The GNOME unlock service stays down | a GNOME lock screen that cannot check a password | "Getting back into a locked session" below |
| `better-lock` crashes | every output black (the lock stays; `cosmic-comp` draws nothing where a lock surface was: `render/mod.rs` lines 1085 to 1112) | the launcher starts it again within a second and `cosmic-comp` accepts the new client's lock (`session_lock.rs` lines 22 to 30); after three crashes in 30 s, `cosmic-greeter`'s locker; if both fail, the steps below |
| `better-lock` hangs | a frozen lock screen | its heartbeat; the launcher restarts it after 30 s of silence |
| The session launcher dies while locked | the compositor's session ends and the login screen appears | logging in again; unsaved work in that session is lost, as on any session crash |

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
   (`better-os-greeter.service` has no `[Install]` section of its own, as
   `cosmic-greeter.service` has none: the alias link is the only thing that
   starts it.)
7. Type `sudo systemctl stop better-os-login-supervisor.service
   better-os-gnome-unlock.service` and Enter, then `sudo systemctl restart
   display-manager.service` and Enter. GDM's login screen appears; if it
   does not, type `sudo reboot` and Enter.
8. Once logged in, open Better Manager: its Better OS session page shows
   "Use the Better OS login screen" off, and it can be removed from there.

Ticket 268 performs these steps by hand in both VMs and records the
screens; Better Manager shows the same text next to the switch.

## Getting back into a locked session

For a session whose lock screen stays black, frozen, or cannot check a
password.

1. Press **Ctrl+Alt+F3** (or F4 to F6).
2. Log in with **the same account** as the locked session, with its
   password.
3. Type `loginctl unlock-session` followed by the session's id from
   `loginctl list-sessions` (the line with your user name and `seat0`), and
   Enter. Both Better OS's lock screen and GNOME's honour it.
4. Switch back with **Ctrl+Alt+F1** (or the terminal `loginctl` lists for
   the session).
5. If logind asks for an administrator's password at step 3, use `sudo
   loginctl unlock-session <id>` instead.
6. If nothing brings the session back, `loginctl terminate-session <id>`
   ends it: **everything not saved in it is lost**, and the login screen
   appears.

In the Better OS session, `better-os-session relock` (ticket 262) starts the
lock program again instead of unlocking, for a person who wants the lock
screen back rather than the session unlocked.

## What an attacker gains

- **At the keyboard:** a password prompt backed by PAM's own rules and
  delay, with the greeter's doubling wait on top; the text fallback is the
  standard `login`. Nothing to open, no launcher, no network menu. The power
  menu can restart the machine, as the power button can. Switching virtual
  terminals shows another login prompt or the locked session's lock, never
  its content (the lock is in the session's compositor in both sessions).
- **Another local account:** can put a crafted avatar or name in front of
  the greeter (decoded by the pure-Rust `image` crate with size limits,
  inside the sandbox); can call the GNOME unlock service and is refused
  unless it is GNOME Shell in its own session asking about itself. A code
  execution in the greeter gains `_better-greeter`: its state directories
  and greetd's socket (trying passwords as anyone at the keyboard can,
  under PAM's delay), nothing else.
- **A program in the person's session:** cannot read the lock program's
  memory, cannot unlock, can draw an imitation over the Better OS lock. It
  can ask the unlock service for a channel only if it is GNOME Shell's own
  executable, and then only to check the person's own password, which
  gains it nothing a password prompt of its own would not.
- **A compromise of the supervisor or the unlock service** is root. That
  is why each has a fixed list of what it may do, its own unit sandbox, no
  input it did not create beyond its D-Bus methods, and tests that hold
  each limit.

## Not proven

- None of the tests named above exists yet; tickets 260 to 268, 280 and
  281 write them.
- That GNOME Shell 46 locks and unlocks through the unlock service as it
  does through GDM, and that GDM's session worker does nothing more on
  reauthentication that the shell relies on (ticket 280).
- Whether the unlock service can run with `PrivateNetwork=yes` on machines
  whose `common-auth` names a networked PAM module.
- Whether logind lets a person unlock their own session from another
  terminal without an administrator's password.
- Whether `cosmic-comp` shows any surface but the lock's while locked, in
  practice (ticket 268 tries a layer surface over a lock).
- Whether `_better-greeter` needs the `video` group under `cosmic-comp`.
- Whether Landlock's TCP rules and the seccomp filter leave GPUI, Orca's
  AT-SPI bus and AccessKit working (ticket 263).
- Every deadline of the supervisor (ticket 281).
