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

- [x] Unit tests: which mounts count as drives (fixture mount tables and
      fake UDisks objects), naming, the bar's thresholds, formatting, a drive
      that does not answer.
- [x] Integration test on a private bus with a fake UDisks2: a drive
      appearing, mounting, filling, ejecting.
- [x] Screenshots from a headless sandbox with fixture drives: sidebar at
      default and minimum width, the tiles view, a nearly full drive, light
      and dark, both languages.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **Which drives.** UDisks2's block devices with a filesystem a person
  keeps files on (ext2/3/4, btrfs, xfs, f2fs, FAT, exFAT, NTFS, HFS+, and
  ISO 9660/UDF for discs), encrypted containers (LUKS, BitLocker) while
  they are locked, the volume inside once unlocked (on the container's
  drive, so a USB one still ejects), GVfs's phones and cameras (`mtp`,
  `gphoto2`, `afc`) and servers, and network shares the kernel mounted
  (`cifs`, `smb3`, `nfs`, `nfs4`, `sshfs`, `davfs`). Left out: loop, RAM
  and zram devices (snaps), swap and other non-filesystems, `tmpfs` and
  every mount under `/boot`, `/efi`, `/recovery`, `/snap`, `/var`, `/usr`,
  `/opt`, `/proc`, `/sys`, `/dev` and `/run` (except `/run/media`),
  partitions labelled EFI, ESP, Recovery, MSR, System Reserved or Boot, and
  an unmounted FAT partition of at most 1 GiB on an internal disk (an EFI
  system partition whatever its label). UDisks2 names one mount point per
  volume, so the drive the mount table shows at `/` is the system drive
  even when UDisks2 named `/home` (btrfs subvolumes). Without UDisks2 the
  mount table alone gives `/` and the media under `/media`, `/run/media`
  and `/mnt`. Read in `crates/files-gui/src/drives.rs` through the storage
  layer's UDisks2 client, unchanged.
- **The row.** The name (label, else "512 GB drive"), an icon for its kind
  (internal disk, USB stick, disc, phone, network), a 4px bar of the space
  in use and "171 GB free of 512 GB" under it, in the units the rest of
  Better Files uses. The bar turns to the caution colour at 90% in use, the
  point at which Windows 11 File Explorer turns a drive's bar red
  (`drives::NEARLY_FULL`). An unmounted drive says its size and "Not
  mounted", a locked one "Encrypted" (the size is left out when the name
  already is it). Removable and optical drives have Eject on the row; the
  rest of the row opens the drive. The kit's `NavItem` gained `meter`.
- **Opening and ejecting.** A mounted drive opens. An unmounted one is
  mounted through UDisks2 off the window's thread (polkit may ask) and
  opens when it is. A locked one asks for its passphrase in the window
  (the same dialog as a server's sign-in); a wrong one says why and asks
  again. Eject unmounts, locks an encrypted container again, and opens a
  disc's tray or powers the drive off where UDisks2 can; a refused unmount or lock is shown and the
  drive stays. A drive the storage service reports keeps its own mount,
  eject and removal state, shown instead of the free space while it needs
  attention.
- **Never blocking.** The drives are read on a thread of their own: at
  start, whenever UDisks2 says a drive came, went or changed, when a file
  operation finishes, after a mount or eject, and every 30 seconds. Each
  drive's free space is read on its own thread and waited for at most one
  second, all at once; a drive that does not answer (a stalled share) is
  listed without a bar and is not asked again until its earlier read comes
  back, so stalled reads never pile up.
- **This PC.** First in Devices; it shows every drive as a tile with a
  larger bar, "This PC" in the address box, and the command bar's file
  commands off. Keys that would act on the folder underneath do nothing
  there; Escape and Back return to it; F5 reads the drives again.
- **Narrow sidebar.** When "171 GB free of 512 GB" would not fit (at the
  200px minimum for most drives), the row keeps its name and bar and drops
  the line (`layout::drive_caption_fits`).
- **Sandbox fixture.** `BETTER_FILES_TEST_DRIVES` names a JSON list of
  drives a sandbox shows instead of the machine's, so screenshots never
  show a real drive.

Tests: `drives.rs` (which UDisks objects and mounts count, locked and
unlocked encrypted volumes, GVfs names, the system drive found through the
mount table, naming and lines in both languages, the 90% threshold, a
drive that does not answer, a stalled share asked once, the narrow
sidebar, ordering); `drives_bus_tests.rs` runs the window's session against
a fake UDisks2 on a private `dbus-daemon`: a stick appears through
InterfacesAdded alone, mounts from its row and opens, fills up past 90%,
ejects (unmount then power-off) and goes when pulled; an encrypted stick
asks for its passphrase, refuses a wrong one, unlocks, mounts, and on eject
unmounts, locks and powers off; a disc is unmounted and its tray opened.

Screenshots in `.shots/`, from a headless GNOME Shell sandbox with fixture
drives (the "Data" drive is 94% full): `t122-sidebar-default-*`,
`t122-sidebar-narrowest-*` (sidebar dragged to its 200px minimum),
`t122-this-pc-*`, `t122-unlock-*`, each `en`/`zh` and `light`/`dark`.
