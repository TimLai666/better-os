# Ticket 179 — A window started from a build directory never starts an installed program

**Blocked by:** none

## Why

Found on 2026-11-21 while photographing ticket 170 in a sandbox. Better
Monitor's window starts its tray by bare name from `PATH`, so a debug window
in a sandbox started the installed `/usr/bin/better-monitor-tray` against the
sandbox's bus and home; two were left running. The same day a sandbox's
private bus activated the installed Better Monitor and Better Awake services
from the system's D-Bus service directories. A window under test then talks
to programs of another version, and screenshots and tests describe a mix
nobody ships.

## What to build

- One rule, in one place, for how a Better OS program finds a companion
  program (a tray, a service, a helper): beside its own executable first, the
  packaged location otherwise, never a bare name resolved through `PATH`.
  Every place that starts a companion uses it (find them all: trays, services
  started on demand, the before-removal and on-disable programs, helpers).
- Under the test guard (ticket 178) nothing installed is started at all.
- The sandbox recipe in the project's notes for a private session bus lists
  only the service directories a test needs, so the system's Better OS
  services cannot be activated there.

## Done when

- Tests: a program run from a build directory starts its sibling from that
  directory and refuses when there is none; with the guard set it starts
  nothing.

## Notes

Built on 2026-11-21.

- The rule is `better_core::companion`. `Finder::locate` looks beside the
  program's own executable first (under the name cargo builds the companion
  as, for a program run from a build directory; under its installed name for
  an installed one), then where the package installs it, and only for a
  program that is itself under `/usr`. A build with no sibling refuses and
  says so. Nothing is looked up through `PATH`. `Finder::plan` says whether a
  companion that is also started by bus or unit name may be asked for by
  name: only by an installed program with the test guard off. Installed
  means one of the packaged directories (`/usr/bin`, `/usr/libexec`,
  `/usr/lib/better-os`); a program anywhere else, `/usr/local/bin`
  included, is a build and starts only a sibling. `COMPANIONS`
  names each program as cargo builds it and as `packaging/build-deb.sh`
  installs it, and a test holds the two together.
- Under the test guard (ticket 178) an installed program starts no companion
  and asks for none by name. A sibling in a build directory is not an
  installed program and still starts; that is what lets a sandbox run of a
  build show its own tray while the guard keeps it off the person's user
  manager.
- Moved onto the rule, every place that started a companion by path:
  - Better Monitor's window, its taskbar item (`monitor-gui` `tray`): an
    installed window asks the bus as before; a build starts the item built
    beside it and never asks the bus, which would have started the installed
    one.
  - Better Awake's window, its taskbar item (`awake-gui` `tray`): the same.
    The old test for a build was "no unit installed", which on a machine with
    Better Awake installed sent a build's request to the installed tray.
  - Better Awake's taskbar item opening Better Manager's Keep awake page and,
    failing that, the old window (`awake-tray` `controller`).
  - Better Monitor's taskbar item opening the popup and the window
    (`monitor-tray` `popup`), directly and through `systemd-run --scope`,
    which now gets the window's path. By bare name this would have started
    `better-monitor`, which in a build directory is the command line.
  - Better Files' large preview (`files-gui` `session::preview_program`),
    which fell back to `better-files-preview` through `PATH`.
  - Better Git's "Show in Files", which looked `better-files` up on `PATH`
    and fell back to `xdg-open`. Review follow-up: it is no longer a
    companion at all. It asks `org.freedesktop.FileManager1`
    (`ShowFolders` for a folder, `ShowItems` for a file;
    `git-gui/src/file_manager.rs`), which is what GNOME calls for "Show in
    folder" and what ticket 129 makes Better Files answer when it is the
    default, so the person's chosen file manager opens and no program is
    started by path or through `PATH`. Under the guard nothing is asked and
    the window says why; with no file manager on the bus the window shows
    the bus's error. The menu entries read "Show in file manager". Tested
    against a fake on a private bus with no service directory.
- Left as they are, with the reason:
  - A program restarting itself after an update (`better_core::self_update`)
    and the daemon's hardware and lamp helpers run their own executable, not
    a companion.
  - Before-removal programs come from dpkg's file list for the installed
    package being removed, under `/usr/lib/better-os/before-removal`; they
    belong to the installed package by design, and under the guard none
    runs.
  - Better Launcher, Better Manager, and the app catalog start applications
    through their desktop entries; that is launching an application, not a
    companion. The daemon-handoff program is started by a maintainer script,
    not by Rust.
- Started by name, which no path can change. What keeps a build from
  reaching the installed program at each is the sandbox bus and the guard,
  not this rule:
  - A component's user units (`manager_platform::user_units`), by unit
    name through `systemctl --user`: the user manager reads the installed
    units. Under the guard nothing is run.
  - D-Bus activation on the session bus of `org.betteros.Monitor1`
    (Better Monitor's window and tray, by their first call),
    `org.betteros.Awake1` (Better Awake's window and tray, Better Manager's
    Keep awake page), `org.betteros.Storage1` (Better Files),
    `org.betteros.WindowAttention1` (Better Manager), and the two trays'
    names for an installed window: a bus started with the system's service
    directories activates the installed program. AGENTS.md's sandbox desktop
    rule now gives the recipe: a bus config whose only `servicedir` holds
    the activation files the test needs.
  - `org.betteros.Launcher1`, which the gesture service calls on a running
    launcher only; it has no activation file.
  - `org.betteros.Manager1` and `org.betteros.Drives1` on the system bus:
    the privileged daemon is the installed one by design, and a build
    reaches it like any client.
- Checks: `better-core` `companion::tests` (a build starts its sibling and
  refuses without one, an installed program finds beside then packaged, the
  guard starts nothing installed, the table matches `build-deb.sh`),
  `companion::tests::a_program_outside_the_packaged_directories_is_a_build`,
  `awake-gui` `tray::tests`, `git-gui` `file_manager::tests`, `files-gui`
  `with_no_preview_program_beside_a_build_nothing_is_started_and_the_pane_opens`.
  A headless GNOME Shell on a bus whose only service directory held dconf's
  activation file ran Better Files and Better Manager from the build
  directory; `busctl --user list` showed no `org.betteros` service but the
  window under test (`/tmp/claude-1000/w77/sandbox/run/log/inner.log`).
