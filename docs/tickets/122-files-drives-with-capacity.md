# Ticket 122 — Better Files' sidebar lists every drive with how full it is

**Blocked by:** none · **Status:** open

## Why

The owner asked on 2026-11-14 to see each drive in the sidebar with its
capacity, as Windows 11 File Explorer's "This PC" does. Today the Devices
section names mounted devices without any size
(`crates/files-gui/src/sidebar.rs`, `SidebarSection::Devices`), and nothing
in Better Files reads free space.

## What to build

- The sidebar's Devices section lists every drive a person would call one:
  the system drive, other internal partitions (mounted or not), removable
  drives, optical and card readers with media, phones and cameras GVfs
  exposes, and mounted network shares; not loop devices, snaps, tmpfs,
  boot/EFI/recovery partitions or other system mounts. The source is the
  storage service or UDisks2 over D-Bus where available (Better Storage
  already talks to it), with the existing mount-table fallback.
- Each drive row shows its name (label, else "<size> drive"), an icon for
  its kind, and a slim capacity bar with "<free> free of <total>" under the
  name; the bar turns to the caution colour when nearly full (pick the
  threshold Windows uses, about 90%, and record it). An unmounted drive shows
  its size and mounts when clicked, asking for a passphrase in the window for
  an encrypted one (ticket 117's sign-in surface). Removable drives have an
  eject button on the row, whole row otherwise clickable.
- Capacity is read off the UI thread, refreshed when a file operation
  finishes, when a drive appears or goes, and on a modest interval while the
  window is visible; a drive that does not answer (a stalled network share)
  shows no bar rather than blocking.
- A "This PC"-style location (Devices, or the existing computer location if
  there is one) shows the same drives as large tiles with the bar in the
  content view.
- The row stays readable at the sidebar's minimum width (ticket 118): the
  text line may be dropped and the bar kept.
- English and Traditional Chinese (Taiwan usage); sizes use the same units
  as the rest of Better Files.

## Acceptance

- [ ] Unit tests: which mounts count as drives (fixture mount tables and
      fake UDisks objects), naming, the bar's thresholds, formatting, a drive
      that does not answer.
- [ ] Integration test on a private bus with a fake UDisks2: a drive
      appearing, mounting, filling, ejecting.
- [ ] Screenshots from a headless sandbox with fixture drives: sidebar at
      default and minimum width, the tiles view, a nearly full drive, light
      and dark, both languages.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
