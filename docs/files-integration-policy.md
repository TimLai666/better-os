# Better Files integration policy

The decisions ticket 35 had to make about the Applications location, Open With,
and external devices, and the limits of each.

## Applications is a view over records, not a place

The location is `Location::Applications`, a variant of the typed location enum.
`Location::as_local_path` answers `None` for it and there is no other accessor,
so the four things Issue #4 forbids are unreachable rather than merely avoided:

- no symlinks are created anywhere
- no `.desktop` file is copied into a directory
- no FUSE filesystem is mounted
- no path is invented that a third-party program could mistake for an executable

A row is an `EntryBody::Application`, which carries a `DesktopId`, an icon
reference, categories, and a comment, and has no path field at all. Opening one
produces `OpenIntent::Launch { desktop_id, action }`, which
`app-catalog-platform` turns into an argument vector built from the registered
desktop definition. Nothing in Better Files builds a command line.

The desktop entry's own file path *is* shown — in the details panel, under a
heading that says "Desktop entry". Issue #4 asks for source metadata to be
revealed for diagnostics; what it forbids is presenting the file as if it were
the application. A labelled diagnostic field that no click acts on is the first
thing and not the second.

## One catalog, and a reload is a whole reload

`CatalogHandle` holds an `Arc<Catalog>` behind an `RwLock`. A listing takes a
snapshot and releases the lock immediately, so a reload cannot block a location
that is already streaming, and a reload during a listing leaves that listing
finishing the catalog it started with.

The watcher never says *which* record changed, and the answer to any change is
therefore a whole reload. That is `app-catalog-platform`'s contract and it is
right: desktop-entry precedence means one new file in `~/.local/share/applications`
can change or reveal a different application entirely. `CatalogWatcher::next_change`
collapses everything inside its settle window into one reload, so installing a
package that writes forty files reloads once.

The catalog carries a generation counter. The window compares it to what it last
drew rather than comparing two catalogs, and reloads the Applications location
when it moves.

A host with no watcher backend keeps a correct catalog that will not notice an
install until the window is reopened. That is a degraded feature reported as
such, not a polling loop added quietly.

## Hidden applications follow the same key as hidden files

`Ctrl+H` reveals `NoDisplay` entries and entries this desktop excluded, because
an application hidden by its own desktop entry is hidden in the same sense a
dotfile is: it exists, and the preference reveals it.

## Open With: one association write path, and it is not here

Double-clicking a file and choosing Open With are the same question, resolved
through one function. The difference is only what happens when there is no
answer.

| Situation | What happens |
| --- | --- |
| `mimeapps.list` names a handler and the catalog has it | launch it with the file as an argument |
| `mimeapps.list` names a handler that is not installed | open the chooser, and say that the previous default is gone |
| nothing is associated | open the chooser |
| the type could not be resolved | say so; do not guess `application/octet-stream` |

The third and fourth rows are separated on purpose. An association that names an
uninstalled application and no association at all look identical to a user and
are not the same problem.

**Better Files writes no association.** The embedded chooser is
`app-chooser-gui`'s own component. Open Once launches the selection and writes
nothing; Always Use goes through `app-chooser-core`'s `AssociationStore`, which
writes its rollback record before the first change and edits a single line in
`mimeapps.list`. That single-line edit is why removing Better Files cannot erase
an unrelated association: there is no code path anywhere in this component that
rewrites the file wholesale, and the rollback record restores exactly the line
that was replaced.

## Devices: service first, this process second, and the window says which

`StorageLink` connects to `org.betteros.Storage1` and proves it by reading the
protocol version — building a proxy for an absent name succeeds, so the property
read is the test. With no service, the same `storage-core` state machine runs in
this process over `storage-platform` events.

The window says which, before it draws a single state. `CollectionMode::InProcess`
is drawn as a **warning**, not a neutral note, and the reason is specific to
storage: an in-process engine sees this application's own writes and the platform
signals it can read, but it does not see the tracked-operation notices another
application would have sent to a service. A readiness claim built without them is
weaker, and the user should know that before trusting a green light.

With neither, `CollectionMode::Unavailable` and the sidebar falls back to the
mount table — which is what a session can honestly report on its own — with
every device reading as "Removal status cannot be verified".

## Devices: the five states, and which two are loud

Issue #5's exact wording, in both shipped languages, compiler-checked because
they are fields of one `Copy` struct:

| State | English | 繁體中文 |
| --- | --- | --- |
| ReadyToUnplug | Ready to unplug | 可以安全拔除 |
| Writing | Writing… Do not unplug | 寫入中… 請勿拔除 |
| Busy | In use by *{app}* | *{app}* 正在使用 |
| PerformanceMode | Performance mode: eject before unplugging | 效能模式：拔除前請先退出 |
| Unknown | Removal status cannot be verified | 無法確認移除狀態 |

A Busy device whose blocker the scan could not name gets "In use by another
application" rather than "In use by " with nothing after it.

Exactly two of the five are drawn as warnings: **Writing**, because unplugging
now loses data, and **Performance mode**, because direct removal is not promised.
Ready, Busy, and Unknown are stated in ordinary text. Issue #5 asks for the idle
state to stay visually quiet and for the sidebar not to become a permanent
warning console, and that is what the split is for.

Only `ReadyToUnplug` permits direct removal. Unknown is never a softer version of
ready.

## Devices: open mounts, leaving does not unmount

Clicking a mounted device navigates into it. Clicking an unmounted one asks the
link to mount it and remembers where it was going; the mount's answer is what
navigates. A mount that fails clears the pending open, so a later mount of the
same device does not navigate somewhere the user has stopped asking for.

Leaving the location asks the link for nothing at all. Eject is a separate,
always-available action, and its report says what actually happened: an unmount
that worked with a power-off that was unavailable is not a clean eject and is not
worded as one.

## Devices: what a disconnect cleans up

When a device leaves, all of this happens without the user doing anything:

1. the sidebar row is removed
2. every remembered location under its mount point is dropped from every pane's
   back and forward stacks, and from every tab's stored history, so reopening a
   closed tab does not bring them back
3. a pane standing on the device navigates to home — and forgets again
   afterwards, because navigating pushes where it was onto the back stack, which
   is the very entry that was just dropped
4. any pending open for it is cancelled

Only two things produce a message. A device disconnected **while being viewed**
says the tab went back to home and why. A device removed **while writing**
produces an unsafe-removal warning that outlives the row, because data that may
not have been written is not a fact to clean up quietly. An idle device unplugged
from a folder nobody is looking at produces nothing, which is the whole point of
Direct Removal.

## What is not wired

- **Performance mode cannot be turned on from Better Files.** The client can set
  the policy and the service refuses it without the acknowledged risks, but no
  UI presents those risks. Issue #5 requires the trade-off to be explained before
  activation, and an explanation is not something to improvise in a sidebar
  context menu.
- **File-operation completion does not notify the storage service.** The client
  method exists and is tested; `files-operations` does not call it, because the
  job engine has no device identity for a destination path. Wiring it means
  mapping a path to a UDisks2 object, which is a `files-operations` change. Until
  it lands, a Better Files copy to an external device is visible to the service
  only through the platform signals, not as a tracked operation.
- **Applications has no category sections and no dedicated grid.** It renders
  through the ordinary content view, which gives it grid and list, sorting,
  keyboard navigation, and search for free. Categories are carried on every
  record and are shown in the details panel; sectioning the view by them is not
  built.
- **Add to Favorites for an application** is not offered, because the bookmarks
  file is a list of URIs and there is no `applications://` URI. Issue #4 defers
  whether that scheme should exist.
