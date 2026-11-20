# Better Patch: the Fixes page

Better Patch finds known Linux desktop faults on this machine, says what is
wrong in plain words, fixes them, checks each fix, and undoes it. It is the
Fixes page of the control center (ADR 0017); the page's layout and copy are
`design/control-center-spec.md` section 9, and the work is ticket 90.

## What an entry is

An entry is typed Rust in `crates/patch-core`, with tests, for a fault someone
reproduced and a fix someone verified on a real machine. Nothing is fetched or
supplied at run time, nothing runs through a shell, and nothing needs root.

Every entry implements `patch_core::Entry`:

| Step | Method | What it does |
| --- | --- | --- |
| Detect | `detection_reads`, `detect` | Names what it must read, then decides from the answers: found (one finding per subject), not present, or cannot check with a reason. |
| Describe | `describe` | The exact change as data (`ChangeLine`), whether undo is needed, and the entry's limits. The page turns these into words. |
| Plan | `plan_reads`, `plan` | Reads again right before the change, then returns the changes (`Change`) and the record of them. |
| Apply | `engine::fix` | Makes the planned changes through the host and keeps the record. |
| Check | `check_reads`, `check` | Reads again after the change: passed, still present, or cannot check. |
| Undo | `undo_reads`, `plan_undo` | Plans the reverse of exactly what the record says, from a fresh reading. |

An entry never touches the machine. The host (`patch_core::Host`,
implemented by `patch_platform::LinuxHost`) answers each `Read` and makes each
`Change`. What the host may run is the closed `patch_core::Command` list: a
fixed absolute path (`/usr/bin/fc-match`, `/usr/bin/fc-cache`,
`/usr/bin/flatpak`) with fixed arguments, plus a validated Flatpak app ID in
one case. A command that changes the machine is never run as a read.
`patch-platform/src/command.rs` is the only place that starts a process; a
test over both crates' source keeps it that way.

### An entry that needs root

The trait does not change. The entry's `plan` returns a new `Change` variant
that names one typed `manager-daemon` method, for example
`Change::Privileged(PrivilegedAction::ReloadUcsi)`. The method gets its own
polkit action and takes no path, device name, or free text, as ADR 0017
requires for every root write. The host sends it over D-Bus. The undo is the
reverse change in `plan_undo`, and the record says what the daemon reported
it did. Adding such an entry touches `Change`, the host's `change`, the
daemon, and its policy file. It never adds a command.

## States

Each subject (the machine, or one app) is in one state, decided by
`patch_core::state::state` from what detection saw, whether a fix is
recorded, and whether it was just undone:

| State | When | zh-TW / English |
| --- | --- | --- |
| Not present | It looked and the fault is not here | 這台電腦沒有這個問題。 / This PC doesn't have this problem. |
| Found | The fault is here, nothing recorded | 這台電腦有這個問題。 / Found on this PC. |
| Fixed and checked | A fix is recorded and the fault is gone | 已修正，檢查通過。 / Fixed and checked. |
| Applied but still present | A fix is recorded and the fault is still there | 已套用，但問題還在。 / Applied, but the problem is still there. |
| Undone | The fix was undone in this window, until Check again | 已復原。 / Undone. |
| Cannot check | A read failed. This outranks a record: a fix whose fault can't be checked is never called fixed | 無法檢查：{reason} / Can't check: {reason} |

A fault can be found and its fix not offered (entry 2 at a fractional scale,
or with screens at different scales). The card says why, in place of the
state, and has no button.

Found and applied-but-still-present need the person: they are the pane's
count and Home's "{n} known problems found". A found fault whose fix is not
offered is listed on the page and not counted, since there is nothing to
press.

The page's description promises only what is true: a fix that changes a
setting can be undone, and a setting the person changes afterwards is left
as they made it (Undo skips it unless they confirm Undo all):
會變更你設定的修正都可以復原；修正之後你自己改過的設定，會保留你改的樣子。 / A fix
that changes your settings can be undone; a setting you change yourself
afterwards is left as you made it. An undo can also fail (the override file
was turned into a link, which is never written through); the page then says
so and the fix stays recorded. The font cache fix changes no setting (it
repairs a cache) and its card says it needs no undo.

## The record

`$XDG_STATE_HOME/better-os/fixes.json` (schema version 1) keeps every
applied fix that can be undone. For an override key it holds the key, the
value the fix wrote, and the value that was there before. It also says
whether the fix created the file and which groups it added.

- Undo puts a key back only while it still holds what the fix wrote. A key
  already back at its earlier value is left alone.
- A key that holds anything else was changed after the fix. It is shown as
  `已由其他程式變更，確認後才會取代` / `Changed by another program; confirm it
  to replace`, the wording and rule Better Defaults uses. Undo leaves it and
  keeps it recorded. **Undo all** is the confirmation that replaces it too.
- Keys the fix never wrote are never in the record, so an undo cannot reach
  anything the person added to the same file.
- A record file that cannot be read, or that a newer Better Manager wrote, is
  reported and never overwritten. The page then offers no Fix or Undo.
- A fix that needs no undo (the font cache) keeps no record. Its result is
  shown until the next Check again.
- The override file is written only while it still holds what the plan read
  (otherwise "changed while it was being fixed"). It is written beside the
  old file and renamed over it, keeping the file's permissions, and it is
  refused when it is a link.
- Editing touches only the lines of the keys the fix owns. Everything else
  stays byte for byte: comments, spacing around `=`, `\r\n` line endings, a
  last line with no newline, repeated keys and repeated groups. Reading
  follows GLib: a repeated group is one group and the last occurrence of a
  key wins, so the fix writes the occurrence that takes effect.
- When the record or the overrides are not at the default places (a custom
  `XDG_STATE_HOME`, `XDG_DATA_HOME`, or `FLATPAK_USER_DIR`), the window
  also keeps `~/.local/state/better-os/fixes-location.json` saying where
  they are, so removing Better Manager can find them; it is removed again
  when everything is at the defaults.

## Entry 1: blank text from a damaged fontconfig cache

Reproduced on 2026-11-11 (ticket 76): every label in an Electron app was
blank, and every font query answered with one file and an unknown style.

- **Detect.** `fc-match -f '%{file}\t%{style[0]}\n'` for `sans-serif`,
  `serif`, `monospace`, `sans-serif:bold`, `serif:italic`, and
  `sans-serif:lang=zh-tw`. The fault is that every answer names the same file
  and no answer has a style (empty or `unknown`). One file with a known style
  is a machine with one font, not a damaged cache. A missing `fc-match` or a
  failed query means cannot check.
- **Fix.** `fc-cache -f`, run as the person, which rebuilds only their own
  cache.
- **Check.** The same queries no longer all give the same file with no style.
- **Undo.** Not needed. The card says 這項修正不需要復原。 / This fix doesn't
  need undoing.

## Entry 2: half-size pointer over an Electron app

Reproduced on 2026-11-11 (ticket 76) with GitHub Desktop, Electron 32.3.0,
as a Flatpak at 200%: the pointer was half size, and running the app
through XWayland fixed it with the picture still sharp.

How Electron chooses Wayland changed, and the entry follows each version:

- Up to 37, Electron follows `ELECTRON_OZONE_PLATFORM_HINT` (`auto` or
  `wayland` puts it on Wayland in a Wayland session).
- In 38 the variable was removed: Electron runs on Wayland when
  `XDG_SESSION_TYPE` is `wayland`. Source: Electron's breaking changes,
  <https://github.com/electron/electron/blob/main/docs/breaking-changes.md>,
  "Removed: `ELECTRON_OZONE_PLATFORM_HINT` environment variable" under
  "Breaking API Changes (38.0)", read 2026-11-21. The fix that made 38 read
  `XDG_SESSION_TYPE` is electron/electron#48301 (merged 2025-09-12).
- The newest releases first look at which display servers are reachable
  (`WAYLAND_SOCKET`, `WAYLAND_DISPLAY`, a `wayland-0` socket; `DISPLAY`),
  use the only one there is, and fall back to `XDG_SESSION_TYPE` when both
  or neither are: `docs/api/command-line-switches.md` and
  electron/electron#54523 (merged 2026-10-08), from Chromium 157 on main and
  backported by hand.

- **Detect.** All of these must hold (when only the Electron version is
  missing, see "cannot tell" below):
  - the session is Wayland (`XDG_SESSION_TYPE`);
  - an installed Flatpak app is Electron: it ships Electron's own `version`
    file beside its `resources.pak` (in its files or up to two folders
    down), or it is built on `org.electronjs.Electron2.BaseApp`;
  - with its metadata and every override layered as Flatpak layers them
    (system global, system app, user global, user app), it is granted the
    `wayland` socket, its `XDG_SESSION_TYPE` is not overridden to something
    else, and: with Electron 38 or later, nothing more; with an older
    Electron, or none known, its `ELECTRON_OZONE_PLATFORM_HINT` is `auto` or
    `wayland`;
  - some logical monitor's scale, from Mutter's
    `org.gnome.Mutter.DisplayConfig.GetCurrentState`, is above 1.

  An app on Flathub's Electron base whose `version` file is not found (it
  is looked for in the app's files and two folders down), with the Wayland
  socket and no hint, is the case where an Electron 38 or later would be on
  Wayland and an older one would not. It is listed as 無法判斷。 / Can't
  tell., with the reason and no Fix, never as "no problem".

  The scale is read only when there is a candidate app, and Mutter has five
  seconds to answer. Outside GNOME (the Better OS session on cosmic-comp is
  coming) nothing reports the scale yet, and the entry says at once
  無法檢查：這個桌面環境還不能提供螢幕縮放比例。 / Can't check: this desktop
  doesn't report its display scale yet. A missing session type, unreadable
  apps, or (with a candidate) an unreadable scale means cannot check.
- **Offered only where verified.** The fix is offered when every monitor
  has the same whole-number scale above 1 (200%, 300%). With a fractional
  scale (125%, 150%) GNOME 46 draws an XWayland window at 1x and stretches
  it, so the whole app turns blurry: the fault is listed with that reason and
  no Fix. With monitors at different whole-number scales (200% and 100%)
  XWayland draws the app at one scale, so it is blurry or the wrong size on
  the other; that case was not verified, and the fault is listed with that
  reason and no Fix either.
- **Fix.** Writes the person's own override file for that app only,
  `$XDG_DATA_HOME/flatpak/overrides/<app-id>`: the same keys `flatpak
  override --user --socket=x11 --nosocket=fallback-x11 --nosocket=wayland
  --env=ELECTRON_OZONE_PLATFORM_HINT=x11 --env=XDG_SESSION_TYPE=x11
  <app-id>` would write, each one doing a part:
  - without the `wayland` socket Flatpak unsets `WAYLAND_DISPLAY` and
    `WAYLAND_SOCKET` and binds no Wayland socket
    (`common/flatpak-run-wayland.c`, `flatpak_run_add_wayland_args`), so the
    newest Electron finds only X11;
  - `XDG_SESSION_TYPE=x11` decides it for Electron 38 and later;
  - `ELECTRON_OZONE_PLATFORM_HINT=x11` decides it up to 37;
  - `x11` granted with `fallback-x11` denied gives the app a `DISPLAY`
    (with `fallback-x11` granted, `--socket=x11` alone leaves it empty).

  Every other line of the file stays as it was. The change takes effect the
  next time the app starts, and the card says so.
- **Screen sharing.** Through XWayland, the app's screen sharing may show
  only its own windows and other X11 apps. WebRTC's
  `DesktopCapturer::IsRunningUnderWayland`
  (`modules/desktop_capture/desktop_capturer.cc`) is true only when
  `XDG_SESSION_TYPE` is `wayland` and `WAYLAND_DISPLAY` is set, and its X11
  screen capturer is used only when that is false
  (`modules/desktop_capture/screen_capturer_linux.cc`); Chromium's
  `CanUsePipeWire()` (`content/public/browser/desktop_capture.cc`) is true
  only in a Wayland session. All three were read on webrtc.googlesource.com
  and chromium.googlesource.com at `main` on 2026-11-21. The fix sets
  `XDG_SESSION_TYPE=x11` and removes `WAYLAND_DISPLAY`, so the app takes the
  X11 capturer, and an X11 capturer under XWayland sees only X clients. It
  has not been tried. The card's cause line says it before Fix is pressed
  (…但分享畫面時可能只看得到 X11 程式的視窗。 / …but its screen sharing may then
  show only X11 windows.), and the limits list says it was read from the
  source and not tried.
- **Check.** `flatpak override --user --show -- <app-id>`, Flatpak's own
  reading of the file, holds every key the fix wrote, and with the app's
  metadata, every other override layer, and its Electron version taken
  together as the app will start, it no longer runs on Wayland.
- **Undo.** Removes exactly those keys, or puts back the values they had. It
  removes the groups the fix added when they are left empty, and the file
  when the fix created it and it is left saying nothing. When every socket
  change is taken back, the `sockets` value goes back exactly as the person
  wrote it (`x11;ssh-auth` stays without a trailing `;`), from the text the
  record kept.

When the fix stops being needed: no Electron release note or issue found on
2026-11-21 says the pointer size on Wayland was fixed (searched
electron/electron issues and pull requests for the pointer and cursor on
Wayland), so the entry cannot tell when the override can go. It reads the
app's Electron version from its `version` file and shows it on the card
(使用 Electron {version}。 / Uses Electron {version}.), and the card says to
undo the fix by hand after the app updates to see whether its pointer is
right on its own.

Limits, which the page states:

- It was verified on a Flatpak only. An app installed any other way is
  offered nothing.
- An app is recognized by Electron's own version file or Flathub's
  Electron base app; one that ships Electron any other way is not.
- Offered only at one whole-number scale above 1, as above.
- Screen sharing from the app may show only X11 windows, as above.
- With no known Electron version and no hint, an app is not reported: an
  older Electron runs through XWayland on its own, and a newer one would
  have shipped its version file.

## Where the page checks

- A real window checks the person's machine.
- A demo window (`BETTER_MANAGER_EXECUTION=mock`) checks a fixture machine
  seeded with both faults under `$XDG_RUNTIME_DIR/better-os/fixes-demo`, so
  a demo never changes this machine. The runtime directory must be a real
  directory the person owns with no access for others, and the two folders
  below it are made 0700 or must be theirs and closed to others' writes
  (`patch_platform::fixture::private_demo_root`). There is no shared fallback
  such as `/tmp`, where another account could lay links in advance for a Fix
  to follow: without a private runtime directory the demo checks nothing and
  says 示範模式找不到只屬於你的資料夾來放範例電腦，所以這裡不做任何檢查。 / Demo
  mode has no private folder for its example PC, so nothing is checked here.
- A debug build takes `BETTER_OS_DEBUG_FIXES_ROOT`, a fixture machine for
  sandbox screenshots (`patch_platform::fixture`).
- A test build checks `/nonexistent/better-os-test-root`, where nothing can
  be read.

The page checks when the window opens (for Home and the count), each time the
page is shown, and on Check again, always off the UI thread. A Fix or an Undo
runs off the UI thread too, and the page checks again afterwards. An update
restart waits while either is running.

A window is often still open while its package is removed. The prerm's
stamp, `/run/better-os/better-manager-removal-run`, is what says a removal
is running: dpkg's selection alone does not, since it stays `deinstall`
after an abandoned removal while the package is `installed`
(`better_core::self_update::removal`). `/run/better-os` exists from boot
(the package's `tmpfiles.d` entry), and the update watcher watches it as
well as the program file, so the window learns of the removal from the
stamp while its own file is still there, and reports
`UpdateState::Removed` (also once the file is gone and dpkg no longer has
the package). The page then changes nothing and says
Better Manager 正在被移除，這一頁現在不會變更任何東西。 / Better Manager is
being removed, so this page changes nothing now. A Fix or an Undo also
looks for the stamp right before it writes, and refuses while it is there,
so nothing is written that the removal has already undone. A removal dpkg
left part way (it asks for `deinstall` and the package is not `installed`,
with no stamp: a power cut) is said, with what finishes it: 移除 Better
Manager 中途停住了。執行「sudo apt install --reinstall better-manager」可以裝回來，
或再移除一次完成移除。 / Removing Better Manager stopped part way. Run "sudo apt
install --reinstall better-manager" to put it back, or remove it again to
finish.

## Removing Better Manager

A fix Better Manager applied is something Better OS added to the machine, so
it does not outlive Better Manager (AGENTS.md). Better Manager cannot remove
itself, so apt is how it goes. Its package ships the before-removal program
`/usr/lib/better-os/before-removal/better-manager`
(`patch_platform::removal`), and its maintainer scripts run it once per
person (`packaging/maintainer/before-removal-accounts.sh`, with
`accounts.sh` and `account-step.sh`):

- **The library.** The account functions (`accounts.sh`,
  `account-step.sh`, the fragment, after their header) are kept in a shell
  variable of the maintainer script, never in a file: nothing in `/tmp` or
  `$TMPDIR` can be swapped for root to run, and a missing or full temporary
  directory fails nothing. It is built only for `prerm remove`, `postinst
  abort-remove`, and `postrm remove|purge`, and neither building it nor
  running it can fail the script. `accounts.sh` documents the interface for
  any other script that runs a small step as each account.
- **Who.** Every account in the login range (`UID_MIN`..`UID_MAX` from
  `/etc/login.defs`, 1000..60000 by default) that `getent passwd` lists, so
  LDAP, sssd, and systemd-homed accounts are reached, whose name is a plain
  name (`[A-Za-z0-9._-]`, not starting with `-`; any other is skipped and
  counted, and nothing of it reaches a program text), and whose home lies,
  by the mount table alone, on a mounted local file system, or on a network
  one while the person has a session (`/run/user/<uid>`). Root never `stat`s
  a home to decide this, so no automount is triggered.
- **Time limits.** Each person's whole step, root's own checks of the home
  included, runs in its own shell under `timeout -s KILL 30`, and the
  listing under the same limit. The whole walk stops after 120 seconds and
  says how many people it did not reach. A home on a dead server costs at
  most one step and a line naming the account; the removal goes on and
  dpkg's lock is let go.
- **How.** As the person (`setpriv`), in a session of its own (`setsid`),
  with standard input from `/dev/null`, its output passed through a loop
  that prefixes each line with the account's name (no `sed` program built
  from data), and only `HOME`, a fixed `PATH`, `LC_ALL=C`, and the run's
  stamp in its environment, so it holds nothing of the terminal apt runs in.
- **prerm remove** first deletes any older stamp (this line comes before
  every other fragment of the prerm, so a later failure of another one can
  never leave an older stamp for abort-remove to read), then makes up a
  stamp for this dpkg run, keeps it in
  `/run/better-os/better-manager-removal-run`, and runs the program. The
  program finds the record and the overrides at the defaults or where the
  location file says, following each path it names only when the deepest
  part of it that exists is the person's own: inside their home, or owned
  by them (a custom `XDG_DATA_HOME` such as `/data/tim/share` is
  followed). It sets the record and each override file a fix
  wrote aside in `~/.local/state/better-os/fixes-removal-undo.json`, stamped.
  Then it undoes every recorded fix that has an undo, through the page's own
  undo path and rule: a key the person changed after the fix is left as they
  made it and named. A fix whose undo fails (the file became a link, changed
  meanwhile, cannot be written) stays recorded and its file is named; the
  rest of the record is deleted. Anything left or named makes it exit 3,
  which is a warning, never a reason to stop the removal.
- **postinst abort-remove** runs it with `--undo` and the stamp: only a
  set-aside file with this run's stamp is put back (every override file and
  the record as they were before the prerm). One with another stamp, or with
  none, is stale and deleted unused.
- **postrm remove or purge** deletes, as the person, the set-aside file,
  the location file, and what an interrupted write of the record, the
  set-aside file, or the location file can leave (`.fixes.json.tmp`,
  `fixes-removal-undo.json.tmp`, `fixes-location.json.tmp`), and the stamp.
  A record that still holds a fix whose undo failed is kept.

A removal that fails after the prerm (dpkg does not call abort-remove then)
leaves Better Manager installed with its fixes undone and the record gone.
At its next start, when no removal is running (no stamp) and dpkg does not
report one stopped part way, the window finds the set-aside file, deletes
it, and says on the Fixes page how many fixes that removal undid. While a
removal runs, the set-aside file is that removal's and is never touched:
an abort-remove's `--undo` needs it (移除 Better Manager 沒有完成，但已經復原了 {n} 項修正。… / Removing Better
Manager didn't finish, and it had already undone {n} fixes. …); each fault
that came back is found again by the next check and can be fixed again.
Nothing is reapplied from a file that might be older than this installation.

A fix with no undo (the rebuilt font cache) keeps no record and is left: it
restored the machine rather than adding to it. The program needs no session,
so it never exits 4.

What removal still does not reach:

- An encrypted home that is not mounted (an fscrypt or ecryptfs home of a
  person who is not logged in, a systemd-homed home that is not active): its
  files cannot be read, so its fixes stay until the person's next login,
  when there is no Better Manager left to undo them. The override is then
  the person's to remove (`flatpak override --user --reset <app-id>` removes
  all of that app's overrides).
- `getent passwd` lists only what the directory service enumerates; sssd
  with enumeration off lists local accounts only.
- Only the user and the system Flatpak installations are read; those listed
  in `/etc/flatpak/installations.d` are not.

The removal has been run only by tests and by a container check in CI
(`packaging/test-daemon-e2e.sh`: an account with uid 1000, a record and an
override, `apt-get remove better-manager` under a time limit); that check's
first run is CI's.

## Candidate faults, not built

Both are recorded in ticket 90 and are not entries, because neither fault is
confirmed yet:

- The panel taking long to light up again on battery after the screen blanks
  (amdgpu PSR). It would be a daemon entry, as above, that sets and restores
  a kernel parameter, with its power cost stated. It waits for the owner's
  trial of `amdgpu.dcdebugmask=0x10`.
- The USB-C port's UCSI interface timing out. Ticket 101 owns it.

## Done when

- Each entry's detection (present and absent), fix, check, and undo are
  tested against a fixture machine in a scratch directory
  (`crates/patch-platform/tests/entries.rs`).
- A test over `patch-core` and `patch-platform` asserts that nothing names a
  shell, that `patch-core` does no I/O, and that only `command.rs` starts a
  process, and only what a `Command` names
  (`crates/patch-platform/tests/no_shell.rs`).
- The page is screenshotted in both appearances and both languages, at 1600
  and 960, with a fault present and with none.
