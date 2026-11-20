# Ticket 100 — Better Files is the file chooser in other applications

**Blocked by:** 93

## Why

The owner asked whether Better Files also replaces the dialog other
applications open to pick a file. It does not yet. On the owner's machine
(read 2026-11-13) `xdg-desktop-portal` prefers the `gnome` backend, then
`gtk`, so applications that ask the portal get GNOME's dialog.

## What to build

- A backend for `org.freedesktop.impl.portal.FileChooser` (OpenFile,
  SaveFile, SaveFiles) served by Better Files, with its `.portal` file and a
  `better-portals.conf` / `gnome-portals.conf` entry that puts it first for
  that interface only, installed and restored by Better Manager.
- The dialog is Better Files' own view in a Fluent content dialog: sidebar,
  breadcrumb, search, grid and list, preview of the selected file, the
  filters and choices the caller passes, current folder and name for save,
  multiple selection, folders-only mode, and keyboard throughout.
- Which applications get it: everything that uses the portal (Flatpak
  applications, GTK 4, Chromium and Electron, Firefox with the portal
  enabled, GPUI). GTK 3 applications draw their own dialog unless
  `GTK_USE_PORTAL=1`; the design says whether Better OS sets that, with the
  risks.
- Tests drive the D-Bus interface with a fake caller; nothing touches the
  person's real portal configuration.

## In the Better OS session (2026-11-21, ADR 0038 option D)

In the session `xdg-desktop-portal` reads `betteros-portals.conf` (the
session's `XDG_CURRENT_DESKTOP` is `BetterOS:COSMIC`), which the session
package ships with `default=cosmic;gtk;` (ticket 203); COSMIC's own file
chooser is what applications get there until this ticket lands. This ticket's
"installed and restored by Better Manager" entry therefore has two halves: on
GNOME, the `gnome-portals.conf` entry as above; in the session, a
`FileChooser=betteros;cosmic;` line in the session's portal configuration
(ticket 203's file, so installing Better Files' backend changes it through
Better Defaults' capture and restore, or the session package ships it once this
backend exists). The dialog, the interface, and the tests are the same in both.
`GTK_USE_PORTAL` is decided once here for both sessions.
