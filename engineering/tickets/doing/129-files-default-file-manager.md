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
- 2026-11-21, built and tested; one thing left. Done: ADR 0041 (how the
  default is held, a running owner left alone, Restore, removal, COSMIC);
  the manifest's `file-manager-service` (`user-service` kind and adapter,
  `boolean: true`, key `org.freedesktop.FileManager1`, `sign-out`), which
  every released manager already accepts; `defaults_platform::bus_activation`
  in the production adapter set; `better-files --service`; the Default apps
  page's words for the row in both languages; `better-files-before-removal`
  shipped as `/usr/lib/better-os/before-removal/better-files`; and
  `crates/defaults-core/tests/bus_activation.rs`, where real `ShowItems`
  calls on a private `dbus-daemon` reach the stand-in Nautilus before, the
  stand-in Better Files after "Use Better OS defaults", Nautilus after
  Restore and after the removal program, a running Nautilus keeps answering
  until it leaves, and a foreign user file comes back byte for byte.
- **Left:** screenshots of the Default apps page showing this row, in zh-TW
  and English, under `/home/tim/Developer/better-os-evidence/129/`. Not
  taken: the disk on this machine fell below the 8 GB floor during the run,
  and the page needs its own manager-gui build plus a private session bus
  and a sandbox home so its production adapters touch nothing of the
  person's. The row's words are tested
  (`the_file_manager_row_names_who_answers_show_in_folder`); how it draws is
  not seen. Also not run: `packaging/build-deb.sh` / `verify-deb.sh` for the
  new before-removal program, and a real GNOME "Show in folder".
