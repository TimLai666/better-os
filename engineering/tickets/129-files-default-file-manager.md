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
