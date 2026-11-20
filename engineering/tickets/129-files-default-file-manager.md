# Ticket 129 — Better Files can be the desktop's file manager

**Blocked by:** none

## Why

Ticket 89 found that the key Better Files' manifest names for "the system
file manager" (`/org/gnome/desktop/applications/file-manager`) does not exist
on GNOME 46. What GNOME actually calls for "Show in folder" and "Open
containing folder" is the `org.freedesktop.FileManager1` D-Bus service, which
Nautilus activates and Better Files already serves when no one else owns it.

## What to build

- Record in an ADR how Better Files becomes the default for
  `org.freedesktop.FileManager1` (a user-level D-Bus activation file that
  takes precedence over Nautilus's, and how a running Nautilus that already
  owns the name is handled), and how Restore puts Nautilus back.
- Retarget Better Files' manifest integration from the dead key to this, with
  the defaults engine's adapter, review wording, restore, and uninstall
  cleanup (the activation file is removed and the previous owner restored).
- Tests against a private session bus: after "Use Better OS defaults" a
  `ShowItems` call reaches Better Files; after Restore it reaches the fake
  Nautilus.

## Notes

- 2026-11-21, from ticket 143's COSMIC VM: COSMIC Files' desktop process takes
  `org.freedesktop.FileManager1` at login, and Better Files then fails to take
  the name. The ADR this ticket writes says what happens on a desktop where
  another program holds the name from the start of the session.
- 2026-11-21, from ticket 134: on GNOME 46 the manifest's
  `file-manager-tool-entry-point` reads `gsettings.no_such_key`, the dead key
  this ticket retargets.
