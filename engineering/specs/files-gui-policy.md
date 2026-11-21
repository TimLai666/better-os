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
a comment, a blank line, a scheme a newer GTK added — is
kept as the exact bytes it was read as, at the exact index it was read at.
Since ticket 310 a server address (`sftp://`, `smb://`, `ftp://`, `dav(s)://`,
`afp://`, `nfs://`) is a favourite of its own, the form Nautilus writes; see
"Remote folders connect when they are opened".
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

## Remote folders connect when they are opened (ticket 310)

Owner, 2026-11-22: a folder on a server (SFTP, SMB and the others GVfs
opens) opens again by itself after its connection has gone — after sleep,
another network, a restarted server or a new login. (On another network
"by itself" is the click on the favourite: nothing is sent there unasked;
see the drop rule below.) Before this ticket a
pinned server folder was stored as the folder GVfs showed it under
(`file:///run/user/<uid>/gvfs/sftp:host=…/path`); once the mount had gone,
the sidebar's availability probe (`path.is_dir()` on the window's thread,
every frame) found nothing there, drew the row "Unavailable", and the click
did nothing. A folder open when its server went away was "deleted" to the
watch hub, which moved the view to GVfs's own folder above it.

**One rule.** `remote::need` is the only place that decides whether a
location needs its connection and whether it is there:

- a server address (`Location::Network`) always needs one, and opens at the
  folder GVfs shows it under once its mount is known;
- a folder under GVfs's folder (`$XDG_RUNTIME_DIR/gvfs/<mount>/…`) whose
  mount is a server's (`sftp`, `ftp`, `smb-share`, `smb-server`, `dav`,
  `dav+sd`, `afp-volume`, `afp-server`, `nfs`) needs one when GVfs's mount
  tracker does not list that mount; its address is read from the mount's
  name (`sftp:host=h,port=p,user=u`, `smb-share:server=h,share=s,…`);
- a server's mount whose name does not give its address (`dav+sd:`, one
  with no host) needs its address asked for once;
- anything else needs nothing. A phone, a camera or an online account under
  GVfs's folder (`mtp:`, `gphoto2:`, `afc:`, `google-drive:`, any kind not
  listed above) is not a server: it keeps the availability any folder has,
  is never asked an address for, and its favourite is never rewritten.

A mount is a favourite's only when it has the same identity: the same
scheme, host (in any case, an IPv6 host with or without its brackets), port,
user, domain and share (an SMB share in any case: `gvfsd` mounts
`smb://NAS/Photos` as `smb-share:server=nas,share=photos`, and writes
`sftp://[::1]:22/` as `sftp:host=%3A%3A1`, both seen in the sandbox, so the
same share written another way is never a second mount). A missing port is
the scheme's own (22, 21, 445, 80, 443, 548, 2049), never "any port"; a
missing user is "no user given", never "any user". A mount on the same host
with another port, user or share is another server's or another account's,
and a favourite never opens, lists or pastes into it.

The reader applies the rule to every listing, so every way of opening a
folder goes through it: a favourite, a recent file's folder, the address
bar, back and forward, a tab restored at start or after an update restart, a
location on the command line or through `org.freedesktop.FileManager1`, a
bookmark another program wrote, a Tailscale machine, and a paste into a
folder that is not connected. A listing that needs a connection fails with
`ListingError::NotConnected`, and the session connects the tab that listed
it. Recent files GIO applications recorded as `sftp://` or `smb://`
addresses (rather than under GVfs's folder) are not listed in Recent; only
`file://` entries are, as before.

**What is stored.** A pinned server folder is the address GVfs needs to
mount it again and the folder in it: scheme, host, port, share (SMB) or
volume (AFP), user (and SMB domain), and path, written as one URI line in the
shared bookmarks file (`sftp://ann@build.local:2222/srv/site`,
`smb://WORKGROUP;ann@nas/media/photos`). An address with no folder
(`sftp://nas`) is the server's own default folder (an SFTP server's home);
one that names the root (`sftp://nas/`) is the root, and each stays what it
is through every form it is written in, a migrated mount-root favourite
included.

**Never a password.** A password typed in an address
(`sftp://ann:secret@nas/srv`) is dropped where the address becomes a
location, as GIO drops it, so it reaches no bookmark, no history entry, no
restart hand-off, no page and no mount request; the server asks for it in
the sign-in form. An empty user (`sftp://@nas/srv`, `sftp://:secret@nas/srv`)
is no user: it goes with its `@` from the stored address, the identity and
the mount spec.

**The old form.** A line an earlier version wrote as a GVfs folder is read
back as the same address by its mount's name, whether that mount is there
or not. The file is written in the new form once a process, when the first
window starts, and only if it still holds what was read; it keeps its mode,
and its new text is written to a temporary file made with that mode, then
renamed over it. One race is left and accepted: a line another program
writes between that check and the rename (milliseconds, once a process) is
lost, because the file has no lock its other writers take.
Nothing else rewrites the shared file without the person doing something to
it: a line another program writes later is read as an address and left as
written. A server mount whose name does not give its address keeps its line
and shows the Address-needed page; only an address typed on that page, for
that favourite, becomes the favourite's (with the folder it named inside
that mount), whether that server still has to be connected or is mounted
already. Leaving the page, or connecting elsewhere, rewrites nothing. The
page names the folder by GVfs's mount name and the path in it
(`dav+sd:host=Cloud._webdav._tcp.local/data`), not by the runtime folder
before it.

**Credentials** are where the first connection put them: GVfs and the
keyring (through Better Files' own mount operation, ticket 117), an SSH agent
or key, a Tailscale machine's own authentication, Kerberos. When the server
asks again, the same sign-in form as a first connection asks, with the same
"Remember" choices. Better Files stores nothing the first connection did
not.

**Connecting.** The tab shows "Connecting to {server}…" with Cancel, in the
content area; the window, the sidebar and every other tab keep working. All
attempts for one mount are one attempt: a second click, a second tab, or a
second favourite on the same server joins the one in flight. Cancel ends the
wait for every tab on it and leaves "Not connected to {server}" with Connect.
A sign-in or host-key question is shown in the tab that is waiting for it.

**When it cannot connect** the page says why, with Try again, and Network
settings where the network is the likely cause (the server did not answer,
its name was not found, no network). Each reason is its own sentence; none
is only "cannot connect". GVfs's codes decide first; a message without one
is read in English, through GVfs's own catalog in the desktop's language,
and for the C library's sentence a backend puts after its own ("Failed to
mount Windows share: No such file or directory", in zh-TW "無法掛載 Windows
檔案共用：沒有此一檔案或目錄") through the C library's own text for that error
in the desktop's language.
The page says what GVfs said and nothing it did not: "No computer called
{server} was found" only when GVfs's own failure says the name was not
found. Better Files does not overrule GVfs with a look of its own, because a
look from here cannot see what ssh and libsmbclient can: an `ssh_config`
alias (`Host`, `HostName`, `Port`, `ProxyJump`) or a NetBIOS name. So
`gvfsd-smb`'s "Invalid argument" for a name that resolves nowhere is "could
not connect" with GVfs's words underneath; a stopped `smbd` gives
"Connection refused" and a missing share "No such file or directory" (all
three seen in the sandbox), and "the share is not there" therefore only comes
from a server that answered. A mount with no share (SFTP) never names one:
it says the folder is not there. A folder that is gone on a server that did
connect opens the nearest folder above it that is there, and says so.

**A connection that drops while a folder is open comes back by itself.**
This is the owner's line above, and the one rule for retries (decided
2026-11-22, replacing the first review round's "stop on another network,
and a server that answers let the mount go on purpose"). A drop is GVfs's
tracker no longer listing the mount, the watch hub reporting the folder
gone, or the folder no longer answering; the tab shows "Connecting to
{server} again…" in place.

- *Let go on purpose, or dropped.* GVfs gives no fact that tells them apart:
  for a `gio mount -u` and for an `sshd` that stopped, the backend calls the
  tracker's `UnregisterMount` and the tracker sends the same `Unmounted`
  (recorded with `dbus-monitor` on the sandbox's bus); the unmount request
  itself travels over GVfs's private peer connection, which no other
  program sees. Better Files unmounts no server itself. So every drop is
  treated as one, and the folder comes back, as the owner asked.
- *No network at the moment* (no default route: a Wi-Fi drop, or a computer
  waking) is waited out with no time limit, and nothing is sent. After 30 s
  of it the page says the computer is not connected to a network, while it
  goes on waiting.
- *The same network back* (the one the folder was last listed on): the host
  is looked at every 5 s (its name resolved, one TCP connection to its port,
  closed at once, nothing sent). A host that answers is connected to at
  once. A look that cannot resolve or reach it cannot tell (an alias, a jump
  host, a NetBIOS name), so GVfs itself is asked to connect every 30 s. For
  30 s the page says "connecting again", then why, while it goes on. After
  10 minutes of trying with a network (long enough for a NAS to restart;
  time without one does not count) it stops, and only a click tries again.
- *Another network*: nothing is sent by itself, because the same name may be
  another computer there and a keyring's password would go to it. The page
  says the connection was lost and this computer is on another network now,
  with Connect; Connect, the favourite, or the address opens it on this
  network, the owner's request for a clicked favourite. Networks are told
  apart by their default routes (interface and gateway), NetworkManager's
  primary connection profile (its UUID, read over the system bus: each Wi-Fi
  network is a profile of its own, so two networks with the same gateway
  address differ), and the gateway's hardware address from the kernel's
  neighbour table (two routers with the same address differ; a wired profile
  shared between networks is told apart by it). They are the same network
  only when that is known: the same routes and profile, and the same router
  where both readings have it; with no NetworkManager the router must be
  known on both sides. What cannot be told is another network.
- A refused sign-in and a changed host key are never tried again by
  themselves, and their page is shown at once.

When the connection is back the tab returns to the same folder, selection
and scroll. A job writing there follows `files-operations`' rules for a
destination that disappears, unchanged.

**A paste that waits.** A paste into a folder that is not connected is held
by the tab and goes in once the folder is connected and listed, whether the
wait ended by a connection or by the folder answering again. It is never
lost without a word: if the folder is gone on the server meanwhile, nothing
is pasted (no folder is made again) and the page says so; if the tab goes
elsewhere (the sidebar, the address bar, back), the connection is cancelled
or left not connected (including a paste whose connection came and whose
folder was not listed yet), stops being tried, or the folder cannot be
listed, the page says at once that nothing was pasted and why, and the
paste never runs later on a Connect. A held cut
empties the clipboard only if the clipboard still holds that cut.

**No waiting on the window's thread.** A dead mount can block a `stat` for
minutes. The mount list is read from GVfs's tracker over the bus on a worker
thread with a 2 s limit; a remote folder is probed on a thread of its own with
a 3 s limit before it is listed, and a mount whose probe has not come back is
known not to answer, so no second probe waits on it; the sidebar decides a
remote favourite's state from the mount list alone and never touches its
folder; a favourite on any other network or FUSE filesystem is probed off
the window's thread; the address bar's check and Tab completion do not read a
server's folder. Every attempt is given up on after GVfs's own 30 s without
news (`network::MOUNT_TIMEOUT`), never while the person is answering a
question.

**Tailscale machines** (ticket 117; ticket 133 adds their SMB shares) open as
the address `sftp://<machine>` (the machine's own default folder, as a click
always opened) and follow the same rule.

**How it is checked.** `remote_tests.rs` drives every entry point, the join,
Cancel, each reason, the migration and the deadlines against a fake
connector and a fake GVfs folder. `remote_servers_tests.rs` starts an
OpenSSH `sshd` (key sign-in, and a second one that takes passwords and can
check none) and a Samba `smbd` unprivileged on loopback, and the installed
`gvfsd` with its SFTP, SMB and FUSE parts on a private bus with its own
`HOME` and runtime folder: a pinned folder opens again after its server is
stopped and started, a drop while open comes back with its selection, a
server that is away, a wrong password and a deleted folder each show their
page, and with the server's processes stopped (holding the connection) the
slowest frame of the window's own work (pump, sidebar rows, a click on the
favourite) measured 1.5 ms over 15 s; it also reads `gvfsd-smb`'s answers for
an unknown host, a stopped server and a missing share, opens the SFTP server
by an alias in the sandbox's own `ssh_config` (opened, dropped and back by
itself, and its failure GVfs's own), and opens a mounted share written in
capitals and an `[::1]` address without a second mount. The network cases
(no network then the same one, sleep, another network) are the fake
connector's, with the network said by the test. CI runs both
real-server tests on its Ubuntu 24.04 runner (FUSE and an unprivileged
`smbd` work there), with `CI` set so a missing program fails rather than
skips. A debug build in a sandbox whose private
bus starts `gvfsd` itself sets `BETTER_FILES_TEST_SANDBOX_GVFS`, which only
drops the check that `gvfsd` runs in `gvfs-daemon.service`; a release build
never reads it.

### What each state says

| State | English | 繁體中文 |
|---|---|---|
| Connecting | Connecting to {server}… — The folder opens as soon as the connection is made. | 正在連線到 {server}… — 連上後就會打開這個資料夾。 |
| Dropped | Connecting to {server} again… — The connection was lost. You'll be back in this folder when it returns. | 正在重新連線到 {server}… — 連線中斷了，恢復後會回到這個資料夾。 |
| Cancelled | Not connected to {server} — This folder is on {server}. Connect to open it. | 尚未連線到 {server} — 這個資料夾在 {server} 上，連線後就能打開。 |
| Dropped, another network | Not connected to {server} — The connection was lost and this computer is on another network now, so it wasn't connected again by itself. Connect to open the folder on {server}. | 尚未連線到 {server} — 連線中斷了，而這台電腦現在連上的是另一個網路，所以沒有自動重新連線。按「連線」就能打開 {server} 上的這個資料夾。 |
| Page title on a failure | Couldn't connect to {server} | 無法連線到 {server} |
| Did not answer | {server} didn't answer. Check that it's turned on and on the network, then try again. | {server} 沒有回應。請確認它已開機並連上網路，再試一次。 |
| Refused | {server} is on the network but didn't accept the connection. Check that its file sharing is turned on. | {server} 在網路上，但沒有接受連線。請確認它的檔案分享已開啟。 |
| Name not found | No computer called {server} was found. Check the name, or connect to the network it's on. | 找不到名為 {server} 的電腦。請檢查名稱，或連上它所在的網路。 |
| Sign-in refused | {server} didn't accept the sign-in. Choose Try again to enter the user name and password again. | {server} 不接受這組登入資料，請按「再試一次」重新輸入。 |
| Share gone | {server} is connected, but {share} isn't shared there any more. It may have been renamed or stopped being shared. | 已連上 {server}，但那裡已經沒有分享 {share}，可能被改名或停止分享了。 |
| Folder gone, no share (SFTP) | {server} answered, but the folder this opens isn't there. | {server} 有回應，但要打開的資料夾不在那裡。 |
| Held paste, tab left | Nothing was pasted into {folder}: you left it before it was connected. | 沒有貼上任何東西到「{folder}」：連上之前你已經離開了這個資料夾。 |
| Held paste, folder deleted | Nothing was pasted: {folder} isn't on {server} any more. | 沒有貼上任何東西：{server} 上已經沒有「{folder}」了。 |
| Held paste, folder not listed | Nothing was pasted into {folder}: it couldn't be opened. | 沒有貼上任何東西到「{folder}」：無法打開這個資料夾。 |
| Held paste, not connected | Nothing was pasted into {folder}: {server} isn't connected. | 沒有貼上任何東西到「{folder}」：沒有連上 {server}。 |
| No network | This computer isn't connected to a network. Connect to one, then try again. | 這台電腦目前沒有連上網路。連上網路後再試一次。 |
| Folder gone, parent opened | {folder} isn't on {server} any more, so the nearest folder above it was opened. | {server} 上已經沒有「{folder}」了，所以打開了上層最接近的資料夾。 |
| Address needed | This folder was on a server whose address wasn't saved. Type its address in the address bar (for example sftp://server/folder) once; the favourite keeps it from then on. | 這個資料夾在一台伺服器上，但當時沒有存下它的位址。請在網址列輸入一次位址（例如 sftp://server/folder），之後這個常用資料夾就會記住它。 |
| Sidebar caption, not connected | Not connected | 未連線 |
| Buttons | Cancel · Connect · Try again · Network settings · Type the address | 取消 · 連線 · 再試一次 · 網路設定 · 輸入位址 |

A changed SSH host key, GVfs missing or not the installed one, an address
GVfs cannot open, and a mount with no folder keep ticket 117's sentences.
Network settings opens `gnome-control-center network`, or
`cosmic-settings network` in the Better OS session, whichever is installed;
without either the button is not shown.

## The sidebar's order

Owner, 2026-11-16 (ticket 132). Top to bottom: the places (Home, the
person's folders, Recent, Trash), Favorites, Applications, Dropbox, each
plugin's section, Devices (This PC and the drives), the tailnet's machines.
The drives sit above the other computers. `sidebar::NAV_ORDER` is the one
place the order is written; the pane draws its groups in that order, and the
keyboard moves through them in the order they are drawn.

## The system drive is named after the running system

Owner, 2026-11-16 (ticket 138). The drive the running system lives on
(`DriveKind::System`, the one mounted at `/`) is called what the running
system calls itself: `NAME` in its os-release ("Zorin OS"), or `PRETTY_NAME`
when there is no `NAME`, whatever its filesystem label says. The file is read
only by `better_core::host` (`/etc/os-release`, and `/usr/lib/os-release`
when that is missing, as os-release(5) orders them). When neither names a
system, the drive is "System drive" / 「系統磁碟」, never a guessed
distribution. Every other drive keeps its label, or is named by its size;
another Linux install on another partition is not looked into.

`drive_mounts::listing::Drive::naming` decides the name, so the sidebar, the
This PC tiles, the path bar's root crumb (which opens `/`), search's "in …"
at `/`, the drive's dialogs and notices, and Better Manager's Drives section
all say the same thing. A storage-service device that the drive listing also
has (`drives::is_device`: the same block or the same mount point, such as the
running system on an external SSD) is drawn as that drive, and its toasts and
confirmations use the drive's name too. No Properties view for a drive exists
yet; one built later takes its name from `Drive::naming`.

## Nothing the running system uses is ejected or formatted

Ticket 171. UDisks2 marks the drive the system runs from only when it is
internal, so a system installed on a USB drive, a live USB, or `/home` or
swap on an external disk is listed by the storage service like any stick.
Whether a drive may be ejected, unmounted, powered off, formatted, or have an
image written to it is decided in one place, `storage_platform::system_use`,
from the kernel's mount table (`/proc/self/mountinfo`), its swap list
(`/proc/swaps`), `/etc/fstab`, and `/sys` (device-mapper and RAID slaves,
a loop device's backing file, a device number's node), matched against
UDisks2's devices.

- **The rule is the right way round.** A drive is the person's only when
  everything of it that is mounted is mounted where removable media is
  mounted for a person: `/media/<name>/…` or `/run/media/<name>/…` where
  `<name>` is an account in the login range (`login.defs`' `UID_MIN` to
  `UID_MAX`, looked up through the system's account database), or Better
  OS's own `/mnt/better-os`. A bare `/media/backup` is the system's. A drive is **held** when any volume on it, or under an
  unlocked container, logical volume or loop file on it (an ISO booted with
  `iso-scan`, a snap), is mounted anywhere else (`/`, `/home/<anyone>`,
  `/srv`, `/opt`, `/nix`, `/var/lib/docker`, `/tmp`, a live medium), holds
  swap in use (a partition, or the filesystem a swap file is on), or is named
  by an `/etc/fstab` line outside Better OS's own block or by the `What=`
  of a mount or automount unit under `/etc/systemd/system` or
  `/run/systemd/system` (a quoted tag, `UUID="…"`, and the
  `/dev/disk/by-*` forms count the same). A ZFS dataset holds
  the drives UDisks2 reports as `zfs_member` of its pool; a bcachefs mount
  holds every member it names; a mount whose source UDisks2 does not know
  (`/dev/root`) is found by its device number. A btrfs filesystem across
  several drives holds all of them.
- A drive is **free** only when the mount table was read, every held mount
  and swap was traced to a drive, and this drive is not one of them.
  Anything else is **unknown**, and unknown is treated as held.
- Better Files' menu for a drive that is held or unknown offers neither
  Eject nor Format and says why in one line, which is also what a refusal
  says when a request was sent anyway: "This drive holds the running system,
  so it can't be ejected or formatted." /
  「這個磁碟上有正在執行的系統，所以不能退出或格式化。」; "This drive holds a
  folder the running system uses, …" / 「這個磁碟上有系統正在使用的資料夾，…」;
  "This drive holds swap the running system uses, …" /
  「這個磁碟上有系統正在使用的置換空間，…」; "The system's mount table
  (/etc/fstab) names this drive, …" / 「系統的掛載表（/etc/fstab）指定了這個
  磁碟，…」; and for unknown "Better Files can't tell whether the running
  system uses this drive, so it can't be ejected or formatted." /
  「無法確定系統是否正在使用這個磁碟，所以不能退出或格式化。」. Write to Drive
  does not list it.
- Better Files reads the rule itself, from its own drive scan, for its own
  rows and for the storage service's (by the UDisks2 drive object a report
  names). The storage service's reports carry no such field: a Better Files
  already released reads them strictly, and a field it does not know would
  make it drop the whole device list. Reports a client reads are no longer
  `deny_unknown_fields` from now on; requests the service reads still are.
  `crates/storage-service/tests/released_client.rs` reads this service's
  replies with v0.2.16's own types.
- The same rule refuses the request where it is carried out: Better Files'
  session, the storage service (`storage.system.running_system`,
  `storage.system.folder:<path>`, `storage.system.swap`,
  `storage.system.table`, `storage.system.unknown`), the direct UDisks2
  eject and unmount of a drive the storage service does not list, Better
  Manager's Drives section, and `manager-daemon`'s `SetMountAutomatically`
  (`system_drive`, which also locks the drive's "Mount automatically" choice
  in both windows). A window mounts or unmounts after a change only when
  the daemon made it.
- **The device confirmed is the device erased.** Format and Write to Drive
  carry the stable identity of the device the person confirmed. The dialog
  closes, saying "The drive was removed or replaced while this was open, so
  nothing was erased." / 「這個視窗開著時磁碟被拔除或換掉了，所以沒有清除任何
  東西。」, when its device goes or another device takes its object path; the
  session compares again before sending, and the service refuses
  (`storage.erase.device_changed`) when the device at that path is another
  one. A service older than the field is asked again without it, after the
  window's own comparison. The identity is read again from the device at
  the moment of action, not from what the service last heard.
- **A device that cannot be told from another is said to be so.** A
  device's identity is stable (a filesystem UUID, partition UUID, serial or
  WWN), weak (only its model, size and port) or volatile (only its kernel
  name). Decided 2026-11-21 (second review of ticket 171): a weak or
  volatile identity may still be formatted or written to, and the
  confirmation names it by what the person can check, label, size and model,
  with "Better Files can't tell this drive from another one like it. Check
  that it is {device} before you erase it." / 「Better Files 無法分辨這個磁碟
  和另一個相同的磁碟。清除之前，請確認它是{device}。」. When another connected
  device has the same identity, any erase is refused
  (`storage.erase.ambiguous`: "Another connected drive looks exactly like
  this one, so Better Files can't be sure which to erase. Unplug the other one
  and try again." / 「另一個已連接的磁碟和這個一模一樣，Better Files 無法確定
  要清除哪一個。請拔除另一個後再試一次。」).

Known limits: a swap file is traced to the drive its filesystem is on, and a
loop file whose backing file is gone leaves every other drive unknown.

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

## Times are shown in local time

The modified and deleted columns format timestamps as `YYYY-MM-DD HH:MM` on the
local wall clock, through `better_core::local_time`, the one conversion every
Better OS program shares (ticket 146). It asks the C library, which reads `TZ`
and `/etc/localtime` and their daylight-saving rules, so each timestamp gets
the offset in force at it. On a machine with no readable zone the time is UTC
and ends in ` UTC`, never a guessed offset. A trash entry's `DeletionDate` is
the trashing person's local time, as the trash specification says and every
file manager writes it, so it is read and written in local time too.

One known effect: Better Files and Better Git 0.2.16 and older wrote that
stamp in UTC. An item they trashed now reads as if the stamp were local, so its
deleted time shows shifted by the machine's UTC offset (eight hours early in
Taipei). Nothing is lost and restore is unaffected, because the stamp only
orders and labels the item. The record carries nothing that tells an old stamp
from a new one, so no entry is guessed at and corrected.

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
would count another program's files. The folder is measured by the same
walk as a folder's size (`files_platform::folder_size`, above): hard links
once, never across a filesystem. A data folder it cannot count in full
(more than 200,000 items, a part it cannot read, or longer than 20 seconds)
shows no Data figure rather than part of one.

Sizes never hold up the listing. The view lists at once, each row reading
"Calculating…" until its size is known; then one thread
(`files-application-sizes`) asks dpkg once for every package and `flatpak`
once per Flatpak, and the view is listed again in place, keeping selection
and scroll. Sizes are kept for ten minutes and forgotten when the catalog
changes. A size sort orders the known sizes and puts every row without one after
them in either direction, as it does a folder without a size.
