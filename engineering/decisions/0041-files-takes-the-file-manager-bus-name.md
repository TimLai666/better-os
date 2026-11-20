# ADR 0041: Better Files becomes the file manager through a user-level D-Bus activation file

## Status

Accepted on 2026-11-21 (ticket 129). It replaces the `file-manager-tool-entry-point`
declaration in `components/manifests/better-files.yaml`, whose key
(`/org/gnome/desktop/applications/file-manager`) does not exist on GNOME 46
(ticket 89, ticket 134). ADR 0009's adapter set gains one production adapter;
the manifest schema is unchanged.

## Context

"Show in folder" in Firefox, Chromium, GNOME Text Editor, a download list or
the desktop portal's `OpenDirectory` is a call to
`org.freedesktop.FileManager1.ShowItems` (or `ShowFolders`,
`ShowItemProperties`) on the session bus. Whoever owns that name answers. No
GSettings key, no `mimeapps.list` line and no desktop entry decides it.

What was read on this machine (Zorin OS 18, GNOME 46), read-only:

- The session bus is the reference `dbus-daemon` 1.14.10, started by systemd
  (`/usr/lib/systemd/user/dbus.service`: `dbus-daemon --session
  --address=systemd: --nofork --nopidfile --systemd-activation`).
  `/usr/share/dbus-1/session.conf` uses `<standard_session_servicedirs/>`.
- Nautilus ships `/usr/share/dbus-1/services/org.freedesktop.FileManager1.service`
  with `Exec=/usr/bin/nautilus --gapplication-service`. Nothing owned the name
  at the time (`busctl --user list` showed it `(activatable)` only): Nautilus
  is started on the first call and is not kept running by the desktop.
- `dbus-daemon(1)`, `<standard_session_servicedirs/>`: the directories are
  searched in the order `$XDG_RUNTIME_DIR/dbus-1/services`,
  `$XDG_DATA_HOME/dbus-1/services` (`~/.local/share` when unset), each
  `$XDG_DATA_DIRS/dbus-1/services`, then `${datadir}/dbus-1/services`; "if a
  particular service is found in more than one service directory, the first
  directory takes precedence". The D-Bus Specification's "Message Bus Starting
  Services" names the same `$XDG_DATA_HOME` before `$XDG_DATA_DIRS` order.
- The bus's own environment (`/proc/<dbus-daemon>/environ`) has no
  `XDG_DATA_HOME`, so the user directory is `~/.local/share/dbus-1/services`;
  it did not exist. The runtime directory is not watched with inotify and
  needs `ReloadConfig`; the other directories are watched, but a directory
  that did not exist when the bus started has no watch. The bus unit's own
  `ExecReload` is a `ReloadConfig` call, which any session client may make.
- A name that already has an owner keeps it. The activation file only says
  what the bus starts when a call finds no owner. Nautilus asks for the name
  without allowing replacement, so a running Nautilus answers until it exits
  (from Nautilus' source, `nautilus-freedesktop-dbus.c`; not exercised here).
- Ticket 143's COSMIC VM: COSMIC Files' desktop process takes the name at
  login, so on that desktop the name always has an owner and activation never
  happens.

## Decision

- **The default is a user-level activation file.** "Use Better OS defaults"
  writes `$XDG_DATA_HOME/dbus-1/services/org.freedesktop.FileManager1.service`
  (the bus's own data directory, `~/.local/share` when unset), exactly:

  ```
  [D-BUS Service]
  Name=org.freedesktop.FileManager1
  Exec=/usr/bin/better-files --service
  ```

  through a temporary file in the same directory and a rename, then calls
  `org.freedesktop.DBus.ReloadConfig` so the bus reads it now whether or not
  the directory was being watched. It shadows Nautilus' file, which stays where
  the package put it. `better-files --service` starts with no window, serves
  the name, opens a window for each call, and leaves when its last window
  closes (or after 30 seconds if no call ever came).
- **It is the `user-service` kind, with the `user-service` adapter.** The
  declaration's key is the bus name; its desired value is `boolean: true`,
  "this component is what the session bus starts for this name". The adapter
  never takes the `Exec` line from the manifest: a fixed table maps the
  component (`better-files`) and the name (`org.freedesktop.FileManager1`) to
  the line above, and any other pair is unsupported. Every released Better
  Manager already accepts `user-service` with a boolean, so the remote
  catalog (ADR 0013) stays readable by them; they have no adapter for it and
  show "needs a manual step", as they did for the dead key.
- **What the adapter reads.** No user file: *unset* (Nautilus' file decides).
  Our exact file: `true` when the name has no owner or its owner's
  executable is `/usr/bin/better-files`; otherwise the owner, as
  `text: "owned-by:<executable>"`. Any other user file: its content, as
  `text`, kept whole so it can be written back byte for byte.
- **A running owner is never stopped.** Applying while Nautilus (or anything
  else) owns the name writes the file and reloads the bus, and leaves the
  running owner alone: it is a program the person may be using, with windows
  open. The declaration's `session_effect` is `sign-out`, so the Default
  apps page shows the change as made and waiting ("takes effect when the
  current file manager closes, at the latest at the next sign-in"), the same
  state Better Defaults already has for a change that is written but not yet
  in force. On GNOME this is rarely seen: Nautilus is started by activation
  and, as a GApplication service, quits after it has been idle with no window
  (how long was not measured here), and nothing at login takes the name. This is how the rule that everything is complete after one
  action in Better Manager holds here: the one action makes every later
  activation Better Files, and nothing else is left for the person to do;
  what is left is a program the person started finishing on its own.
- **Restore puts back exactly what was there.** The capture before the first
  change is either *unset* or the foreign file's content. Restore removes our
  file, or writes the captured content back, then reloads the bus; a running
  Better Files keeps answering until it exits, and the next call starts
  whatever the restored files say. A file someone else wrote over ours after
  the change is *changed externally* and is not touched without the person
  confirming it, as for every other default.
- **Uninstalling Better Files restores it.** The package ships
  `/usr/lib/better-os/before-removal/better-files`, which manager-core runs as
  the person before the package goes (the mechanism ticket 88 added for
  Better Touchpad). It runs the same restore for this integration, from the
  person's defaults snapshot; with no snapshot it removes the file only if it
  is byte for byte ours. Removal never fails because of it.
- **A desktop where another program holds the name from the start of the
  session is not offered.** The declaration lists `sessions: [gnome]`; COSMIC,
  where COSMIC Files' desktop takes the name at login and keeps it, reports
  the integration unavailable rather than writing a file that could never be
  used. Supporting such a desktop needs that desktop's own setting, in a
  later decision.

## Consequences

- The person's choice lives in one file in their own home, which any user can
  inspect and which a reinstall of Nautilus does not undo.
- `$XDG_RUNTIME_DIR/dbus-1/services` takes precedence over the user file. A
  session that puts its own file there wins, and the adapter reads that as
  another owner when it answers a call.
- A running Better Files after a Restore keeps answering until it exits; it
  is never killed, for the same reason Nautilus is not.
- The bus is asked which process owns the name (`GetNameOwner`,
  `GetConnectionUnixProcessID`, then `/proc/<pid>/exe`). A process the person
  does not own may hide its executable; the reading is then *unknown*, never
  guessed to be Better Files.

## Rejected options

- **Killing or asking Nautilus to quit.** It may hold open windows and copy
  jobs; taking the name from it would lose them.
- **Replacing the name at runtime from a running Better Files.** Nautilus does
  not allow replacement, and a Better Files that queues for the name owns it
  only while it runs, so "Show in folder" would depend on Better Files being
  open.
- **Editing or diverting Nautilus' file under `/usr/share`.** It belongs to the
  `nautilus` package, needs root, and is undone by every Nautilus update.
- **`$XDG_RUNTIME_DIR/dbus-1/services`.** It is cleared at every sign-out, so
  the default would not survive one.
