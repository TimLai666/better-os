# Better Files GUI policy

The decisions ticket 34 had to make, and why. Issue #6 leaves each of these
open or asks for an explicit policy rather than a silent choice.

## View preferences are global, per user

One file holds the view mode, the sort key and direction, folders-first, the
item scale, the hidden-entry preference, and the language:

```
$XDG_CONFIG_HOME/better-os/files/view.json
```

Every tab is opened from it and every change is written back to it. There is no
per-folder store.

The reason is reversibility. `files_core::ViewPreferences` already lives on the
tab, so a per-folder rule can be added later by writing a different value into
one tab; nothing in the model or the file has to change. Shipping per-folder
first would have been the irreversible choice, because a directory-keyed store
has to decide eviction, what an unwritable or read-only directory does, and what
a moved or renamed directory inherits — and none of those is answerable before
there is a window to observe people using.

A missing file is the defaults. A corrupt file is the defaults plus a notice the
window shows once, rather than a silent revert of settings the user chose.

## Bookmarks live in the shared XDG file

Favorites are stored in:

```
$XDG_CONFIG_HOME/gtk-3.0/bookmarks
```

One URI per line, with an optional display label after a space. Nautilus, Nemo,
Thunar, and PCManFM read the same file, so a folder pinned in Better Files is
pinned in the session's other file managers too.

Three consequences are deliberate.

**Labels go in the shared file, not in a side-car.** The format has a place for
them, so there is no private store to drift out of step with the shared one.

**Foreign lines survive byte for byte.** A line this build does not understand —
an `sftp://` bookmark, a comment, a blank line, a scheme a newer GTK added — is
kept as the exact bytes it was read as, at the exact index it was read at.
Reordering swaps our bookmarks between the positions our bookmarks already
occupy, so a foreign line never moves and never disappears. This is the same
rule Better App Chooser applies to `mimeapps.list`, for the same reason: the
file belongs to the desktop, not to us.

**A missing target is a state, not a deletion.** A bookmark whose folder is gone
stays in the list, drawn as unavailable, and the file on disk is untouched. "The
disk is not plugged in right now" and "the user wants this gone" are different
sentences, and only the second one is an edit.

## Pinning: drag, and a context action beside it

Dropping a directory from the content area onto the Favorites section creates a
bookmark and never moves the directory. That is a real GPUI drag: the content
row carries a typed payload and the Favorites drop zone accepts it.

A pointer drag is not reachable from a keyboard or a screen reader, so the same
action is also a right-click on a content row, a click on the Favorites drop
zone (which pins the current location), and `Alt+Up` / `Alt+Down` / `Delete` on
a focused favourite for reorder and removal. Issue #6 asks for the drag *and*
for keyboard-accessible alternatives; both exist, and neither is a stand-in for
the other.

Dragging a favourite between two other favourites to reorder is modelled
(`BookmarkFile::move_to`) and reachable from the buttons and the keyboard. The
pointer reorder gesture inside the sidebar is not wired yet.

## Devices without the storage service

Ticket 35 connected the Devices section to the storage layer, and
`engineering/specs/files-integration-policy.md` records how. What remains true from this
ticket is the fallback underneath it.

When neither the session service nor an in-process engine can produce device
states, the section is built from the mount table, which is what a session can
honestly report on its own: which filesystems are mounted, which look external,
and how much their identity is worth. A device whose identity is only a kernel
name is drawn with "identity valid for this connection only", because that is
what `storage_core::IdentityConfidence::Volatile` means.

Every device in that fallback reads as `DeviceStateKind::Unknown`, which never
reads as safe to unplug.

## Drives that are not mounted are not shown

Ticket 127, ADR 0032. Every drive the sidebar would list with a filesystem
UUID has a "Mount automatically" choice, in its context menu and in Better
Manager's System page (Drives), which read and change the same thing.

- **Defaults** (owner, 2026-11-15): the system drive is always on and cannot
  be changed; removable drives and discs are on; another internal partition
  is on only when the system mounts it at boot today, because of a line in
  `/etc/fstab` someone else wrote (then the choice is locked and says so) or
  because Better OS was asked to.
- **The sidebar and "This PC" list** a drive that is mounted, an internal
  drive that is on and did not mount (with the reason systemd gave and
  Retry), a removable drive that is on and not yet mounted ("Not mounted",
  mounts when clicked: the desktop mounts it on arrival), and a locked
  encrypted drive that is on. A drive that is off and not mounted is not
  listed.
- **"Show other drives"** in the window's `…` menu, the place Better Files
  keeps its settings, lists every drive, those that are off included, so one
  can be turned back on from its context menu. It is remembered with the
  other preferences.
- A drive with no filesystem UUID has no choice (the choice is kept by
  UUID); it is listed while mounted, and otherwise under "Show other
  drives", where it says "Not mounted" and mounts when clicked.
- Turning a removable drive on mounts it now through UDisks2; turning a
  drive off unmounts a mount UDisks2 made. A mount under `/mnt/better-os` is
  the daemon's, and the daemon unmounts it.
- A change that is refused, dismissed at the password prompt, or fails is
  said in the window with its reason; a dismissed prompt says nothing.

## Times are shown in UTC

The modified column formats timestamps as `YYYY-MM-DD HH:MM` in UTC. There is no
time-zone database in this workspace, and deriving one from `TZ` alone would be
wrong for half the year in most of the world. This is the one display value that
is not yet in the user's local time; fixing it needs a decision about a time-zone
dependency.

## Opening a file is a typed intent, not a launch

`files_core::open_intent` decides what opening an entry means, as a closed enum
rather than a path: a directory navigates, an application launches through its
desktop id, a file opens with whatever its association resolves to. Ticket 35
connected the second and third to `app-catalog-platform` and Better App Chooser;
`engineering/specs/files-integration-policy.md` records how. There is still no second
launcher and no second association lookup in this crate, which is the property
the typed intent exists to keep.

## The GUI does not run as root

`better-files` reads its own effective uid from `/proc/self/status` before
opening a window and exits if it is 0. There is no code path that elevates, and
every filesystem change goes through the `files-operations` job engine as the
session user.

## Folder sizes (ticket 126)

A folder's Size cell, its row in the details panel, the preview's size line,
the grid tile's caption, and the status bar's total for a selection all show
the folder's total: the sum of the regular files beneath it, each hard-linked
file counted once, without following a symbolic link and without entering
another filesystem (`files_platform::folder_size`). A symbolic link adds
nothing; a folder's own directory blocks add nothing.

- **When.** Nothing is measured until the folder's listing is in, so the
  listing and its first frame are not slowed. The walks run on one thread of
  their own, one folder after another in the order the view lists them
  (`files_gui::folder_sizes`). Until a folder's answer arrives its cell says
  "Calculating…" / "計算中…".
- **Sorting.** A measured total is written into the model as the entry's size,
  so sorting by size uses it; a folder without a number sorts after every
  known size in both directions, and moves into place when its number arrives.
  Selection is by identity, so it stays on the same entries.
- **Bounds.** A folder with more than 1,000,000 entries, or still being walked
  20 seconds after it started, says "More than N items" / "超過 N 個項目", with N
  what was counted. One visit to a folder may look at 5,000,000 entries in 120
  seconds across all its folders; a folder never reached says "—" and why.
- **Leaving.** Leaving the folder cancels its walks between two entries and
  drops their answers.
- **What is not walked.** Folders on a network filesystem or a FUSE mount
  (`MountTable::reach`, from the mount table alone), and everything in the
  Trash, say "—" with the reason in a tooltip. "Show folder sizes" in the View
  menu turns all of it off (folders say "—", and the sizes are taken out of the
  model so a size sort is what it was); "Folder sizes on network locations"
  turns walking network and FUSE folders on. Both are stored with the other
  view settings (`folder_sizes`, `folder_sizes_on_network`). Better Files has
  no separate Settings window, so the View menu is where they live.
- **Unreadable.** A folder that cannot be listed says "Can't read" / "無法讀取",
  never 0. A folder with unreadable folders inside shows "≥ total" with how
  many could not be read in its tooltip, and sorts by the part it could read.
- **Cache.** An answer is kept for five minutes. The folder watcher forgets
  it sooner: a change the open folder's watch reports forgets the changed
  folder, the open folder, and every cached folder above it; and each measured
  folder of the visit (up to 256) has a watch of its own, so a file added
  directly inside it is seen while its total is on screen.

**Limit.** A change two or more levels inside a folder that is not open is not
seen by any watch (one inotify watch per folder in a tree would exhaust the
user's limit), so such a total can be up to five minutes old when the person
comes back to it. A change made between a walk finishing and that folder's own
watch being placed (a few milliseconds) is missed the same way.
