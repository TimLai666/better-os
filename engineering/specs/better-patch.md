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

Found and applied-but-still-present need the person: they are the pane's
count and Home's "{n} known problems found".

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
  old file and renamed over it, and it is refused when it is a link.

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

Reproduced on 2026-11-11 (ticket 76) with an Electron 32 Flatpak at 200%.

- **Detect.** All of these must hold:
  - the session is Wayland (`XDG_SESSION_TYPE`);
  - an installed Flatpak app is built on `org.electronjs.Electron2.BaseApp`
    (its metadata's `[Application] base`);
  - with its metadata and every override layered as Flatpak layers them
    (system global, system app, user global, user app), it is granted the
    `wayland` socket and its `ELECTRON_OZONE_PLATFORM_HINT` is `auto` or
    `wayland`;
  - some logical monitor's scale, from Mutter's
    `org.gnome.Mutter.DisplayConfig.GetCurrentState`, is above 1.

  The scale is read only when there is a candidate app. A missing session
  type, unreadable apps, or (with a candidate) an unreadable scale means
  cannot check.
- **Fix.** Writes the person's own override file for that app only,
  `$XDG_DATA_HOME/flatpak/overrides/<app-id>`. It is the same file
  `flatpak override --user --nosocket=fallback-x11 --socket=x11
  --env=ELECTRON_OZONE_PLATFORM_HINT=x11 <app-id>` would write, and every
  other line stays as it was. The change takes effect the next time the app
  starts, and the card says so.
- **Check.** `flatpak override --user --show -- <app-id>`, Flatpak's own
  reading of the file, holds every key the fix wrote.
- **Undo.** Removes exactly those keys, or puts back the values they had. It
  removes the groups the fix added when they are left empty, and the file
  when the fix created it and it is left saying nothing.

Limits, which the page states:

- It was verified on a Flatpak only. An app installed any other way is
  offered nothing.
- Only apps built on Flathub's Electron base app are recognized.
- An app with no Electron hint is not reported. Older Electron then runs
  through XWayland on its own. Whether a newer Electron that defaults to
  Wayland shows the fault is unverified.
- The override should be undone once the app ships an Electron that scales
  its pointer. Nothing here can tell when that happens, so the card says it.

## Where the page checks

- A real window checks the person's machine.
- A demo window (`BETTER_MANAGER_EXECUTION=mock`) checks a fixture machine
  seeded with both faults under `$XDG_RUNTIME_DIR/better-os/fixes-demo`, so
  a demo never changes this machine.
- A debug build takes `BETTER_OS_DEBUG_FIXES_ROOT`, a fixture machine for
  sandbox screenshots (`patch_platform::fixture`).
- A test build checks `/nonexistent/better-os-test-root`, where nothing can
  be read.

The page checks when the window opens (for Home and the count), each time the
page is shown, and on Check again, always off the UI thread. A Fix or an Undo
runs off the UI thread too, and the page checks again afterwards. An update
restart waits while either is running.

## Removing Better Manager

A fix Better Manager applied is something Better OS added to the machine, so
it does not outlive Better Manager (AGENTS.md). Better Manager cannot remove
itself, so apt is how it goes. Its package ships the before-removal program
`/usr/lib/better-os/before-removal/better-manager`
(`patch_platform::removal`), and its maintainer scripts run it once per
person (`packaging/maintainer/before-removal-accounts.sh`):

- **prerm remove** runs the program as each person, with only `HOME`, a
  fixed `PATH` and `LC_ALL=C` in its environment. It sets the record and each
  override file a fix wrote aside in
  `~/.local/state/better-os/fixes-removal-undo.json`. Then it undoes every
  recorded fix that has an undo, through the page's own undo path and rule:
  a key the person changed after the fix is left as they made it and named
  on the terminal, and the program exits 3. Last, it deletes the record.
- **postinst abort-remove** runs it with `--undo`: every override file and
  the record go back to what they were before the prerm.
- **postrm remove or purge** deletes the set-aside file, as the person.

A fix with no undo (the rebuilt font cache) keeps no record and is left: it
restored the machine rather than adding to it. The program needs no session,
so it never exits 4. A record it cannot read is kept, not deleted, and the
program exits 3. A person whose `XDG_DATA_HOME` or `XDG_STATE_HOME` is not
the default keeps their fixes, because a maintainer script cannot see those
variables. Only the user and the system Flatpak installations are read;
those listed in `/etc/flatpak/installations.d` are not.

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
