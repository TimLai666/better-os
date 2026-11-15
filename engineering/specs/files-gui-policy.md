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

## Application sizes

The Applications view (ticket 128) shows how much space each application
takes, in the Size column, its grid tile, the Properties panel, and the
application details panel. Every figure comes from a record the system
already keeps, read without privilege (`files_platform::app_size`):

| Where it came from | What the size is |
| --- | --- |
| A Flatpak (its entry resolves into `flatpak/app/<id>/…`, or it runs `flatpak`) | `flatpak info --show-size`, the app alone. The runtime named in the deployment's `metadata` is shared with other apps and not added; the panel names it. |
| A Snap (its entry is in snapd's desktop folder) | The current revision's `/var/lib/snapd/snaps/<name>_<rev>.snap`, the revision `/snap/<name>/current` points at. |
| An AppImage, or a program in the person's home | The program file's size. |
| Anything else | dpkg's `Installed-Size` of the package `dpkg-query -S` names as the desktop entry's owner. |

A Debian application is its package alone, and the note under its size says
so. Adding the dependencies only it uses was considered and not done: dpkg
records what a package depends on, not which of those nothing else needs,
and apt's "installed automatically" mark is not that either, so any total
would be a guess.

When there is no record, the cell is "—" and its tooltip, and the panels,
give the reason: no installed package owns the entry, more than one does,
the package records no size, Flatpak did not answer, the snap revision is
gone, the program file is gone, or dpkg could not be asked. No size is ever
filled in from a default.

The person's own data for an application is never part of its size. It is
shown as a separate "Data" figure only where its folder is known for
certain: a Flatpak's `~/.var/app/<id>` and a snap's `~/snap/<name>`. A native
application's `~/.config` folder has no recorded name, and guessing one
would count another program's files. A data folder of more than 200,000
items shows no Data figure rather than part of one.

Sizes never hold up the listing. The view lists at once, each row reading
"Calculating…" until its size is known; then one thread
(`files-application-sizes`) asks dpkg once for every package and `flatpak`
once per Flatpak, and the view is listed again in place, keeping selection
and scroll. Sizes are kept for ten minutes and forgotten when the catalog
changes. A size sort orders the known sizes; rows without one sort together, after
them in ascending order and before them in descending order, as a folder
without a size does.
