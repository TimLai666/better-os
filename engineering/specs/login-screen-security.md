# The login screen and the lock screen: security review

The review the Better OS greeter (`better-greeter` under greetd) and the
Better OS lock program (`better-lock` in the session) need before either runs
on a person's machine, and before the greeter may replace GDM
([ADR 0044](/engineering/decisions/0044-better-os-login-screen-and-gdm.md),
proposed). The design is `design/login-screen-spec.md` (ticket 259). Every
claim below names the source, check or test that holds it, or the ticket
that will add the test; a claim with none is listed under "Not proven".
Nothing here has run yet: the tests named are the ones tickets 260 to 268
must write.

## What it does

- **Login.** At boot and after every logout, `greetd` (root) opens a PAM
  session of class `greeter` for the account `_better-greeter` and runs
  `/usr/sbin/better-os-greeter`, which execs `cosmic-comp` with
  `better-greeter` as its program. The greeter shows the accounts
  AccountsService lists, takes a password, hands it to greetd over greetd's
  socket, and on success asks greetd to start the chosen session, then
  exits. greetd runs PAM (`/etc/pam.d/greetd`) and starts the session as
  the person.
- **Lock.** In the Better OS session, the session launcher starts
  `better-lock` as the person, inside the session's scope. It takes the
  compositor's session lock (`ext_session_lock_v1`), covers every output,
  and runs PAM itself under the service `better-os-lock` until the person's
  password is accepted, then unlocks and exits.

## The risk being reviewed

The login screen is the one program on the machine that every person
passes through and that runs before anyone is authenticated. The lock
screen is the only thing between a person's open session and whoever is at
the keyboard. So the review has to establish, in this order:

1. a password never leaves the path keyboard → program → PAM, is never kept,
   and never reaches a log, a core file, the network, or another account;
2. neither program can be made to unlock or log in without PAM saying yes;
3. a crash or a hang of either never leaves the person unable to reach a
   login or their session, and never leaves a session unlocked;
4. the greeter reads nothing a person controls beyond what GDM already
   reads, and nothing at all from a home directory;
5. the way back to GDM works from a text console with nothing of Better OS
   running.

## Threat model

- **A person at the keyboard of a locked or logged-out machine,** who does
  not know the password. They may type anything, plug in a keyboard or a
  USB device, close and open the lid, unplug a monitor, and press keys at
  any speed.
- **Another local account** on the same machine, who can set their own
  avatar, real name and session in AccountsService, and so can put a crafted
  image and long or strange text in front of the greeter.
- **A program in the person's session** (the person's own, or one that got
  in), which can talk to the session bus and the compositor but does not
  know the password. It may try to draw over the lock screen, to read the
  lock program's memory, or to ask it to unlock.
- Out of scope: root, physical access to the disk, the kernel, firmware.
  Whoever has root can replace either program.

## Who holds which privilege

| Process | Account | Started by | Holds |
| --- | --- | --- | --- |
| `greetd` 0.9.0 | root | `better-os-greeter.service` | PAM for both services, the VT, the socket it gives to the greeter (`greetd/src/server.rs` line 182 `chown`s it to the greeter's account) |
| `/usr/sbin/better-os-greeter` | `_better-greeter` | greetd's greeter session | nothing; sets the environment and execs the compositor |
| `cosmic-comp` (greeter) | `_better-greeter` | the line above | the seat's DRM and input devices through logind (class `greeter` session, active on `seat0`) |
| `better-greeter` | `_better-greeter` | `cosmic-comp` | greetd's socket; the system bus as an unprivileged client; read and write in `/var/lib/better-os-greeter/` only |
| Orca, AT-SPI bus (only with the screen reader on) | `_better-greeter` | the start wrapper | the greeter's own D-Bus session; reads the greeter's accessibility tree |
| `better-lock` | the person | the session launcher | the person's PAM conversation under `better-os-lock`; the compositor's session lock; the session bus |
| `cosmic-greeter` locker (fallback only) | the person | the session launcher after three crashes | the same, under its own PAM service |

`_better-greeter` is a system account created by sysusers: no password, no
shell (`/usr/sbin/nologin`), home `/var/lib/better-os-greeter` (0700),
member of `video` only if `cosmic-comp` needs it there (to be checked in
ticket 268; `cosmic-greeter`'s account is in `video`,
`debian/cosmic-greeter.sysusers`). It owns no file outside its home.

## What each can reach

`better-greeter`, after its sandbox is applied (ticket 263; applied in
`main` before the first D-Bus call or file read):

| Resource | Allowed | How it is held |
| --- | --- | --- |
| greetd's socket | connect, read, write | `GREETD_SOCK`; the only Unix stream socket it opens besides the system bus and the Wayland socket |
| System bus | method calls and property reads on AccountsService, logind (`Can*`, `Suspend`, `Reboot`, `PowerOff`, `ListSessions`), NetworkManager and UPower (properties only), locale1 | the bus's own policy; logind's power calls are decided by polkit for an active greeter session; no other destination is called (a test lists every destination in the code) |
| Files read | `/usr/share/` (artwork, fonts, `wayland-sessions`), `/etc/fonts`, `/var/lib/AccountsService/icons/`, its own state directory, `$XDG_RUNTIME_DIR` | Landlock (ABI 4, Linux 6.7 and newer; Ubuntu 24.04 ships 6.8) |
| Files written | its state directory and `$XDG_RUNTIME_DIR` | Landlock; files created new with `O_NOFOLLOW` and renamed into place |
| Network | none | Landlock's TCP bind and connect rules, and a seccomp filter refusing `socket(AF_INET, …)`, `AF_INET6`, `AF_PACKET`; a test runs the built binary and checks that a `connect` attempt from inside fails |
| Other processes | none | seccomp refuses `ptrace`, `process_vm_readv`, `process_vm_writev`; `PR_SET_DUMPABLE` is 0, so no other process of the same account can attach and no core file holds memory |
| Executing programs | none after start | seccomp refuses `execve` once the screen is up; Orca is started by the wrapper before |

`better-lock` runs with the person's rights, so a program of the person's
could always read its memory if it could attach. It therefore sets
`PR_SET_DUMPABLE` 0 at start (so `ptrace` from the same account is refused
without `CAP_SYS_PTRACE`) and uses the same no-network and no-`execve` rules.
It reads the person's wallpaper, the session's settings and notification
history through the session bus; that is the person's own data in their own
session.

## The password

- **Where it lives.** One fixed-capacity buffer per prompt (256 bytes, as
  `pam_unix` reads at most `PAM_MAX_RESP_SIZE` 512 and Better OS's field
  stops at 256 characters), allocated once, locked in memory with
  `mlock` so it is never swapped, owned by `login-screen-core`'s `Secret`
  type, which implements no `Debug`, `Display`, `Clone` or `Serialize`, and
  wipes itself with `zeroize` on drop.
- **Where it goes.** In the greeter, into greetd's
  `post_auth_message_response` JSON, built directly into a second `Secret`
  buffer (never a `String` or a `serde_json::Value`), written to the socket,
  then both are wiped. In the lock program, into PAM's conversation reply,
  which PAM frees itself; the program's copy is wiped when the reply is
  handed over.
- **When it is wiped.** After it is sent; on Escape; when the view changes
  (quiet, chooser, another account); after 60 seconds without input; when
  the program exits normally. The field's drawn text is bullets only,
  except while "Show password" is held on.
- **What is never done with it.** No log line, no error message, no panic
  message carries it; a test feeds a known secret through the whole path
  with logging at its most verbose and searches the log and every error
  value for it (ticket 263).
- **Input methods.** The greeter runs none. The lock surface takes the
  keyboard from the session; the field asks for the `password` content
  purpose and the lock program keeps text input disabled, so Fcitx 5 is not
  handed the keys (ticket 201 saw the keys reach the locker, not the input
  method, with `cosmic-greeter`; ticket 262 repeats it with `better-lock`).

## It never unlocks without PAM

- The greeter has no way to start a session without greetd's `success` for
  that account: greetd refuses `start_session` unless the session is ready
  (`context.rs`, `start`: `SessionState::Question` is an error).
- The lock program calls `unlock_and_destroy` only from the one code path
  that follows PAM's `authenticate` and `acct_mgmt` both returning success
  for the session's own user; the rule is in `login-screen-core` with a test
  that every other outcome (error, conversation failure, PAM module missing,
  a panic in the conversation) leaves the lock held.
- No D-Bus method of the lock program unlocks. logind's `Unlock` signal is
  honoured only by ending the program's own view when the compositor reports
  the lock finished; it never calls `unlock_and_destroy` on its own.
- Nothing draws over the lock: `cosmic-comp` shows only lock surfaces while
  locked, and the one exception, `cosmic_session_lock_layer_manager_v1`, is
  offered only to clients that are not sandboxed (`src/state.rs` line 776),
  which is any of the person's own programs. A program of the person's could
  therefore put a layer over the lock screen and imitate it. That is the
  same exposure as every Wayland session where the person's programs are not
  sandboxed; it cannot unlock, because only the lock client's
  `unlock_and_destroy` does that. Listed under "Not proven" until ticket 268
  checks which surfaces `cosmic-comp` shows above a lock.

## What a crash or a hang leaves the person with

| What fails | What the person sees | What brings it back |
| --- | --- | --- |
| `better-greeter` crashes | the compositor exits with it; greetd stops ("greeter exited without creating a session", `context.rs` lines 357 to 360); a second of text console | systemd restarts the unit (`Restart=always`, `RestartSec=1`); the fifth failure within 30 s fails the unit, and `OnFailure=gdm.service` starts GDM for this boot. The next start of Better Manager reports it (ticket 267) |
| `better-greeter` hangs (alive, not drawing or not answering) | a frozen login screen | greetd cannot tell. The greeter writes a heartbeat to `/run/better-os-greeter/alive` every 5 s; the login screen watch (ticket 267) reads it, and if it is 30 s old with no person's session on the seat, restarts `display-manager.service` once and, if the heartbeat stays stale, starts GDM in its place |
| `cosmic-comp` fails to start (the greeter's) | text console | as the first row: the greeter's start wrapper exits, greetd stops, the unit's limit brings GDM |
| greetd crashes | text console | `Restart=always`, then GDM as above |
| `better-lock` crashes | every output black (the protocol keeps the session locked, and `cosmic-comp` draws nothing where a lock surface was: `render/mod.rs` lines 1085 to 1112) | the launcher starts it again within a second, and `cosmic-comp` accepts the new client's lock because the dead client is gone (`session_lock.rs` lines 22 to 30); after three crashes in 30 s it starts `cosmic-greeter`'s locker instead. If both fail, the steps under "Getting back into a locked session" |
| `better-lock` hangs | a frozen lock screen | the person switches to a text console and follows "Getting back into a locked session"; the launcher has no way to tell a slow lock program from a hung one without one more heartbeat, which ticket 262 adds (the same 5 s file under `$XDG_RUNTIME_DIR`), restarting a lock program silent for 30 s |
| The session launcher dies while locked | the compositor's session ends and greetd shows the greeter (or GDM) | logging in again; unsaved work in that session is lost, as on any session crash |

**ADR 0033's watch today does not cover greetd.** It polls
`display-manager.service` and, when it "resolves to another display manager,
the watch does nothing and says so in `last-boot.json`", and the only units
it will restart are `gdm.service` and `gdm3.service`. Ticket 267 adds the
greetd form: the same once-per-boot rule, the greeter's session of class
`greeter` on `seat0` as "the login screen", the heartbeat above for a hang,
and GDM as the second step instead of a second restart.

## The way back to GDM from a text console

For a machine where the Better OS login screen does not appear and GDM did
not take over by itself. Every step works with nothing of Better OS
running.

1. Press **Ctrl+Alt+F3**. If the screen stays black, try **Ctrl+Alt+F4**
   to **F6**.
2. At `login:`, type the user name of an account that can use `sudo` (the
   first account made on the machine), press Enter, then its password and
   Enter. Nothing is shown while the password is typed.
3. Type `sudo dpkg-reconfigure gdm3` and press Enter, then the password
   again.
4. A list titled "Default display manager" appears. Move to **gdm3** with
   the arrow keys and press **Enter**.
   (If no list appears, GDM was already chosen: go to step 6.)
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
7. Type `sudo systemctl restart display-manager.service` and Enter. GDM's
   login screen appears; if it does not, type `sudo reboot` and Enter.
8. Once logged in, open Better Manager: its Better OS session page shows
   "Use the Better OS login screen" off, and it can be removed from there.

Ticket 268 performs these steps by hand in both VMs and records the
screens; the copy of these steps that Better Manager shows next to the
switch is the same text.

## Getting back into a locked session

For a Better OS session whose screen stays black or frozen while locked.

1. Press **Ctrl+Alt+F3** (or F4 to F6).
2. Log in with **the same account** as the locked session.
3. Type `better-os-session relock` and Enter. It asks the session launcher
   (over the account's own session bus, which the text console shares) to
   start the lock program again; switch back with **Ctrl+Alt+F1** (or the
   key of the session's VT, which `loginctl` lists) and unlock as usual.
4. If that does not bring the lock screen back, `loginctl terminate-session
   <id>` with the session's id from `loginctl list-sessions` ends the
   session: **everything not saved in it is lost**, and the login screen
   appears.

`better-os-session relock` is ticket 262's; it refuses when there is no
Better OS session of the account, and it never unlocks.

## What an attacker gains

- **At the keyboard:** a password prompt backed by PAM's own rules and
  delay, with the greeter's doubling wait on top. Nothing to open, no
  launcher, no network menu, no file chooser. The power menu can restart
  the machine, as the power button can.
- **Another local account:** can put a crafted avatar or name in front of
  the greeter. The avatar is decoded by the pure-Rust `image` crate with
  size limits inside the sandbox; a crash costs that boot's greeter and
  falls back as above; a code execution gains `_better-greeter`, which can
  read the greeter's state directory, talk to greetd's socket (it could try
  passwords, as anyone at the keyboard can, under PAM's delay), and nothing
  else. It cannot read another person's files or reach the network.
- **A program in the person's session:** cannot read the lock program's
  memory (`PR_SET_DUMPABLE`), cannot unlock, can draw an imitation over it
  (above). It could already read everything of the person's.

## Not proven

- None of the tests named above exists yet; tickets 260 to 268 write them.
- Whether `cosmic-comp` shows any surface but the lock's while locked, in
  practice (ticket 268 tries a layer surface over a lock).
- Whether `_better-greeter` needs the `video` group under `cosmic-comp`.
- Whether Landlock's TCP rules and the seccomp filter leave GPUI, Orca's
  AT-SPI bus and AccessKit working (ticket 263).
- `OnFailure=gdm.service` taking over after the fifth failure (ticket 268).
- GNOME having no lock screen under greetd is read from GNOME Shell 46's
  source and was not run.
