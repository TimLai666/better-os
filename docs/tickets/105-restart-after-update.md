# Ticket 105 — After an update, everything runs the new version

**Blocked by:** none · **Status:** merged, not released

## Why

The owner, 2026-11-14: after a Better OS app is updated, it only behaves
like the new version once it is restarted by hand. Updating should restart
what it replaced, background services included, so no part of Better OS
keeps running the old program next to new files and settings.

What restarts today:

- A component's systemd user units are enabled and restarted by Better
  Manager when it installs, updates, or restores the component
  (`manager-platform/src/user_units.rs`, `enable_and_restart`). Ticket 102
  and 97 turn the Awake and Monitor tray icons into user units, so they are
  covered when updated through Better Manager.
- Nothing restarts an open application window. After an update it keeps
  running the deleted old binary (`/proc/<pid>/exe` ends in `(deleted)`),
  and can talk to a restarted service with a newer protocol (ticket 97 bumped
  Better Monitor's).
- Nothing restarts anything when the packages are updated outside Better
  Manager (`apt upgrade`, Software Updater, unattended upgrades).
- `better-manager-daemon` (the system service) is reloaded by its
  maintainer script, but whether it is restarted on upgrade has to be
  checked.

## What to build

- **One rule, in AGENTS.md.** After any update of a Better OS package,
  however it was installed, every Better OS process of that package runs the
  new program within a few seconds: services, tray icons, and open windows.
- **Services and trays, whatever updated them.** Each package's maintainer
  scripts restart its system units on upgrade (`deb-systemd-invoke
  try-restart` or the debhelper equivalent) and, for user units, ask every
  logged-in user's manager to restart them
  (`systemctl --user -M <uid>@ try-restart` or `systemd-run --user` per
  user, only units that are running, never starting a unit the person
  turned off). Better Manager keeps doing its own restart, and the two
  never fight (try-restart is idempotent). A user unit that is new in the
  upgraded package (ticket 102's `better-awake-tray.service`, ticket 97's
  `better-monitor-tray.service`, which replace autostart entries) is enabled
  and started for each logged-in user when the component was enabled
  before, so an upgrade with `apt` does not leave the person without the
  tray until Better Manager next runs.
- **Windows restart themselves and come back where they were.** Every app
  watches its own program file (`/proc/self/exe` replaced, or an inotify on
  the installed path). When it changes, the app saves its view state (open
  folder, repository, page, tab, selection, scroll, window size and place,
  and any text being typed such as a commit message), starts the new
  program with that state, and exits; the new window replaces the old one in
  place. If the app is in the middle of something that cannot be carried
  over (a running file operation, a Git operation, an open dialog with
  unsaved input that has no saved form), it waits until that finishes and
  then restarts, showing a small "Updated — restarting when this finishes"
  note meanwhile. A shared `better-ui` helper does the watching, state
  hand-off, and re-exec, so every app uses the same code.
- **Better Manager restarting itself.** When Better Manager updates itself
  it finishes the transaction, records the result, restarts into the new
  version, and shows the result page.
- **Visible, not silent.** After a restart the window shows "Updated to
  {version}" for a few seconds (the same short-lived notice as ticket 103's
  toasts), never a modal.

## Acceptance

- [ ] In a disposable VM or container with a session: update each component
      both through Better Manager and with `apt`, with its window open and
      its services running; afterwards no Better OS process has a
      `(deleted)` executable, every window is back with its state, and a
      service the person had turned off stays off. Record the checks.
- [ ] Unit tests for the state save and restore of each app, the "wait until
      the operation finishes" rule, and the maintainer scripts' per-user
      restart (with a fake systemctl).
- [ ] Tests never touch the person's real systemd user manager or windows.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      `build-deb.sh` + `verify-deb.sh` for all nine packages, and the
      license check pass.

## What was built

**One rule, in AGENTS.md.** The working rules now say that after any update,
however it was installed, every Better OS process of the package runs the new
program within a few seconds, and name the three mechanisms below; a new
process must use one of them.

**The shared logic, with no GPUI in it: `better_core::self_update`.**
`ProgramFile` remembers the path a process was started from and the inode
that is running (`stat` through `/proc/self/exe` still reaches a deleted
file). `UpdateTracker` turns the path's state into *current*, *in progress*,
or *ready*: a new file at the path is only ready once dpkg reports the
package configured at a version different from the one the program started
with (so a window never starts before the package's postinst has restarted
the service it talks to). dpkg is asked through `dpkg-query`, not by reading
`/var/lib/dpkg/status`: while dpkg runs, its changes sit in
`/var/lib/dpkg/updates/` and the status file can still say the old version is
installed while the new one is half-configured (`dpkg-query` applies that
journal, tested against a scratch admin directory). It is asked again only
when the status file or the journal directory changed. A reinstall of the
same version settles three seconds after it is configured, a development
build with no package two seconds after its file stops changing, and a dpkg
run that died half way gives up waiting after five minutes. `decide` is the one waiting rule: ready and
busy waits, ready and idle restarts. `Handoff` and `HandoffDir` carry the view
in `$XDG_RUNTIME_DIR/better-os/restart/<app id>-<pid>.json`: the directory must
be a real directory owned by the user (mode forced to 0700), the file is
created new with `O_EXCL|O_NOFOLLOW`, mode 0600, and renamed into place, it
is read once through `O_NOFOLLOW`, at most 1 MiB, and refused when older than
60 seconds, for another program, or another process; a state the new program
cannot read costs the view and keeps the window size and the notice.
`restart_in_place` `exec`s the new program with the original arguments, so
the process id — and therefore the hand-off's name and the desktop's view of
the window — stay right. The D-Bus single-instance names (Better Manager,
Better Launcher) are released when the old connection closes on exec; a
restarted window that still finds its name taken asks again for up to three
seconds before treating it as another window's. `ProgramWatch` watches the program's
directory through `notify` (inotify), so nothing polls between updates.


**The window side: `better_ui::restart`.** `UpdateRestart` is mounted by every
window and drives the tracker from the watch; `Restartable` is what a window
implements — its view, and what blocks a restart as a `Blocker` (work running,
a dialog, or changes to save or discard). `take_handoff` and `window_bounds`
reopen the window at its size and maximized or fullscreen state (Wayland gives
no position). The notices, in English and zh-TW: "Updated to {version}" for
six seconds after a restart; while waiting, "An update is ready — Better X
will restart when you close this dialog / when this task finishes / when you
save or discard your changes"; and, if the exec itself fails, "A new version
is installed — close this window and open it again". The last two can be
closed; a closed waiting notice comes back only for a different reason.

**One toast for every window: `better_ui::fluent::toast`.** The notices are
toasts on the window's one `ToastHost` (`UpdateRestart::toast_host`), which
the window can use for its own toasts too: a queue (three shown, the rest
waiting, a timed toast's clock starting when it is shown, and a timed toast
shown anyway above the limit when the three shown all stay until closed, so it
is never starved), cards at the
bottom right drawn above dialogs (a deferred layer at priority 3, so a toast
about a dialog is not dimmed by the dialog's smoke), an optional action
button (a link or an undo, which also dismisses the toast), an optional close
button, and one timer rule — the clock stops while the pointer is on a toast
and resumes with what was left, never less than a second. The queue and the
timer have no GPUI in them and are tested. Better Git's own notices (a pull
request or issue created, published, forked, a stash waiting, checks re-run,
signed in), which ticket 103 drew with a toast of its own, now go through this
host too: the link and the stash's "restore" are the toast's action, a new
notice replaces the one shown, and a toast that goes away clears the notice;
103's problem bar is unchanged. `better-fluent-gallery` shows the
cards. `UpdateRestart::inactive` is what a window built by a test or a
benchmark gets; it watches nothing and still has a host.

**Each window**, with its view and blocking rule in a GPUI-free `restart`
module and tests:

| Window | Carried over | Waits while |
| --- | --- | --- |
| Better Git | repository, Changes/History, selected file or commit, filter, the whole commit form (summary, description, co-authors, amend and the other options), settings page and section, filter and options panes, branch flyout tab, diff scroll | a git command runs or waits, a background fetch runs, any dialog is open, or the settings page's name, email, or default branch differ from git's configuration (saved with Save, or put back with the new Revert beside it) |
| Better Files | every tab's location (device locations excepted), the front tab, the selection with its cursor, the search text, the path being typed, the operations panel, content scroll | a file operation is queued, running, or paused, a Taildrop or LocalSend send is under way in the process (counted for the process, so closing the window that started it does not let another window restart under it), a dialog, the Open With chooser, or the Performance mode confirmation is open — in the window or in its split view's right pane, which does not watch for updates itself |
| Better Manager | page and back history, component and detail tab, activity filter, search and "find a setting" text, pane state, About, update-check state, scroll, and the plan under review or just finished | a transaction runs (`transaction_running()`), a repair or the startup unit pass runs, or the installation screen is showing |
| Better Monitor | page, Performance resource, per-core, pause, pane width, filter, every table's sort, selection, expanded apps, history window, settings expanders | a question (such as "End this process?") is open |
| Better Touchpad | page, the device whose gesture profile is edited, both file paths | settings are applying or moved but not applied, a preview, an import, the gesture editor, or a live test is open |
| Better Awake | section | a rule is being edited or a request to the service is in flight |
| Better Launcher | what was typed and whether All apps was open | never |

`transaction_running()` was stuck true after the first transaction in a
window — the transaction's task was kept and never cleared, which also left
the repair button disabled, skipped the startup unit pass, and sent Back to a
finished progress screen. The background work now sends one "ended" message
however it ends — the privileged service unreachable, a result, an error part
way, or a panic (`runner_end::EndsOnce`) — and the window lets the task go
then, leaving the installation screen for Apps when no finish was reported.

Better Manager updating itself is this same path: the window keeps the old
program while the transaction runs, the runner records the result in the state
file, the window lands on the result page, restarts, and the new window opens
on the result page for the carried plan with "Updated to …".

**The trays.** Better Awake's and Better Monitor's tray icons are systemd user
units since tickets 102 and 97, so the package postinst below restarts them
like any service, and a tray does not watch itself. (A first draft of this
ticket had the Awake tray `exec` itself when its file changed, from when it was
an autostart entry; with the unit there would have been two mechanisms for one
process, and that code was removed.)

**Services, whatever updated them.** `packaging/maintainer/user-units.sh` is
copied by `build-deb.sh` into the preinst and postinst of every package that
ships a user unit (monitor, touchpad, awake, storage). The preinst records
which of the package's units the installed version already had; the postinst,
for every person whose user manager is running (`/run/user/<uid>/systemd/private`),
has systemd start `systemctl --user` for them (`systemd-run --wait --collect
-p User=<uid> -p RuntimeMaxSec=30 -E XDG_RUNTIME_DIR=/run/user/<uid>`; `--wait`
returns the command's exit status, and its output goes to the journal):
a fresh process from PID 1 with nothing inherited from the maintainer script,
and no PAM session, so no login record, no message of the day, and no
dependency on dbus-user-session (`systemctl --machine=<uid>@` would open a
PAM login session per call). It does: `daemon-reload`, `try-restart --no-block` of the existing units —
which never starts a stopped one — and `enable --now` of a unit that is new in
this version only for a person who has one of the component's other units
enabled, which is what ticket 102's and 97's tray units need. Better Manager's
privileged service runs with `ProtectHome=yes`, which hides `/run/user`, so a
postinst running inside `better-manager-daemon.service` runs itself again
outside it with `systemd-run --wait`. Nothing fails a package: every user step
is `|| true`. `verify-deb.sh` checks every package that ships a user unit has
both scripts, names exactly its units, and parses as `sh`.

**`better-manager-daemon`.** Checked, as the ticket asked: its prerm stopped
the unit on every invocation, including an upgrade, and nothing restarted it.
Reproduced in a VM: run from inside `better-manager-daemon.service` (where the
daemon runs apt), the 0.2.12 prerm's `systemctl stop` killed the dpkg run
executing it and left the package half-installed (`iFR`). The prerm now stops
the unit only on remove or deconfigure and never from inside the unit; the
postinst `try-restart`s a running daemon after an upgrade from outside, but
not while that daemon is in a transaction or a repair: the daemon creates
`/run/better-os/daemon-busy` when one starts and removes it when it ends, and
the postinst looks at nothing else (`packaging/maintainer/daemon.sh`; apt can
get in between two steps of a plan). The daemon removes a marker left by one
that died when it starts, and `/run` is emptied at boot. The running slot, the
call's place inside the gate, and the marker all go with the work on its
blocking thread rather than with the D-Bus call, so a client that disconnects
mid-transaction leaves the daemon busy until dpkg ends (tested over a private
bus). The daemon, once its program was
replaced, leaves ten seconds after its last transaction instead of 120
(`wait_until_idle_or_replaced`), so D-Bus starts the new one. It leaves by
closing a gate every call goes through: a new call is refused with the
retryable `daemon.error.shutting_down`, which Better Manager's client asks
again for up to ten seconds (by then a new daemon answers); the calls already
inside finish; and only then does it give the bus name up and exit. Giving the
name up first would not work: for a `Type=dbus` unit, systemd treats losing the
name as the service stopping and signals it at once. One call can still be cut
off: staging a downloaded package holds no transaction, so an outside upgrade
may restart the daemon under it. Better Manager tells that apart from a
refusal and says so — "An update restarted the Better OS system service during
this step. Try again." (zh-TW too) — rather than reporting a failed install. Better Manager has no manifest for the daemon, so it never updates it
through itself today; if a future plan did, an update *from* 0.2.12 or older
would still run that version's prerm, which no new package can change.

**Verified** in a disposable Ubuntu 24.04 VM (qemu/KVM, cloud image, user
`tester` logged in, a headless sway session, lavapipe), with all nine packages
built from this tree as 0.2.12 → 0.2.13 → 0.2.14 → 0.2.15 and the 0.2.12
daemon carrying the published maintainer scripts:

- apt, 0.2.12 → 0.2.13, all seven windows open and three user services running:
  every window restarted in place (same pid, executable no longer deleted), Git
  came back on its repository with the half-typed summary, Files on its folder,
  Manager on Settings; the three services had new pids running the new program.
- Better Manager's privileged service (driven by the e2e client, the same
  `DbusPrivilegedExecutor` the window uses), 0.2.14 → 0.2.15 for all nine
  packages, the daemon itself last, with the Better Manager window closed and
  the touchpad gestures unit turned off: every window restarted, every running
  service restarted, the gestures unit stayed disabled and inactive, the daemon
  updated itself to `ii` and exited.
- apt reinstall of the daemon while it ran: restarted with a new pid.
- After the review fixes, apt 0.2.15 → 0.2.16 → 0.2.17 with the windows open
  (the second upgrade decided by the `dpkg-query` tracker): every window and
  service on the new program, Git back with its half-typed summary, and the
  turned-off gestures unit still disabled and inactive.
- After the second review, apt 0.2.17 → 0.2.18 with tester logged in: the
  three running user services restarted on the new program, the turned-off
  gestures unit stayed disabled and inactive, `last`, `lastlog -u tester`, and
  `loginctl list-sessions` were unchanged, the journal held no PAM session from
  the upgrade (only the test's own ssh and sudo), and each per-user call ran as
  a transient `run-u*.service` started by PID 1.
- An upgrade of the daemon package landing while the daemon runs a 20-second
  transaction (0.2.19): `/run/better-os/daemon-busy` existed during it, the
  postinst left the daemon alone (same main pid), the transaction finished
  successfully, the marker was gone afterwards, and the same postinst then
  restarted the idle daemon (new pid).
- Better Git's Settings → Git page with Save and the new Revert, English dark
  and zh-TW light.
- Better Git in English and zh-TW, light and dark: "Updated to 0.2.17" at the
  bottom right after a reinstall; with the Clone dialog open, "An update is
  ready — Better Git will restart when you close this dialog", drawn above the
  dialog's smoke with a close button, and the old program kept until the dialog
  closed.

The disposable VM ran with 2 GB of RAM inside a 4 GB memory scope.

What was not seen live: the tray restart (the VM's sway has no
StatusNotifierWatcher, so the tray exits before it would watch), a unit that is
new in an upgraded package (no shipped package has one yet; the fake-systemctl
test covers it), Better Manager restarting itself in the middle of its own
transaction through the window, and GNOME placing a restarted window — sway
puts it on the focused workspace, and a Wayland client cannot choose its
position.
