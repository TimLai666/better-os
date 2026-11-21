# Ticket 310 — Better Files connects a remote folder again when it is opened

**Spec:** the Better Files network section this ticket adds to (find the file
under `engineering/specs/` that owns SFTP and SMB locations; tickets 133 and
137 name it)
**Blocked by:** none

## Why

Owner, 2026-11-22: a folder on a network drive (SFTP, SMB, and the like) is
pinned to Favourites. After the connection has gone (sleep, another network,
the server restarted, a new login), clicking it in the sidebar only says it
cannot be reached. Opening a folder that was reached through a remote
connection must make that connection again by itself.

## What to build

- **Every way of opening a location connects it first when its connection is
  gone**: a sidebar favourite, a recent folder, the address bar, back and
  forward, a tab restored at start, a restored window after an update restart
  (`Restartable`), a location handed in on the command line or through
  `org.freedesktop.FileManager1`, a bookmark from GTK's bookmarks file, and a
  drop target or paste target that names a remote folder. One rule in one
  place decides "this location needs its connection and it is not there"; no
  entry point has its own copy.
- **What is remembered with a pinned or recent remote folder** is enough to
  connect again: the scheme, host, port, share, user name and path, never a
  password. Favourites and recents written by earlier versions that hold only
  a mounted path (a gvfs FUSE path under `/run/user/<uid>/gvfs/`, or a path
  under a mount Better Files made) are migrated on read by resolving the mount
  they sat on while it is known, and are otherwise shown as needing a
  connection with the address asked for once.
- **Credentials**: taken from where the first connection put them (the
  keyring through the mount operation, an SSH agent or key, a Tailscale
  machine's own authentication, Kerberos); when the server asks again, the
  same password dialog as a first connection, with "Remember" as it is there.
  Nothing is stored that the first connection did not store.
- **While it connects** the view shows that it is connecting to the named
  server, with Cancel; the window stays usable and other tabs are not held.
  A second click on the same favourite joins the attempt in flight; it does
  not start another.
- **When it cannot connect** the page says why in words a person can act on
  (the server did not answer, the name was not found, sign-in was refused,
  the share or folder no longer exists, the network is down), with Try again
  and, where it helps, the Network settings; never only "cannot connect". A
  folder that is gone on a server that did connect opens the nearest parent
  that exists and says so.
- **A connection that drops while a folder is open** shows the same connecting
  state in place and comes back to the same folder, selection and scroll when
  it returns; a job in flight to that location follows the rules
  `files-operations` already has for a destination that disappears, and this
  ticket does not change them.
- **No waiting on the interface thread**: a dead mount can block `stat` for a
  long time. Every probe of a remote location has a deadline and runs off the
  interface thread; the sidebar never freezes because a favourite's server is
  away, at start or later.
- **Tailscale machines** (ticket 133: clicking a machine opens SFTP, its SMB
  shares listed under it) follow the same rule.

## Done when

- [x] The spec states the rule, the stored form of a remote favourite, every
      state and its sentences in zh-TW and English.
- [x] Tests on the GPUI-free model for every entry point above, the join of a
      second attempt, cancel, each failure reason, the migration of old
      favourites, and the deadline; against a fake connector.
- [x] A check against real servers in the sandbox: an SFTP server and an SMB
      server started by the test itself (in the brief's sandbox, on loopback,
      with fixture accounts), a folder pinned, the server stopped and started
      again, and the favourite opening the folder with no further action;
      wrong password, server away, and folder deleted each showing their
      page. Screenshots light and dark, zh-TW and English, under
      `better-os-evidence/310/`.
- [x] The sidebar and the window stay responsive with a favourite whose
      server silently drops packets (a firewalled loopback port or a stopped
      process holding the socket), measured.

## Notes

**Root cause (2026-11-22, read in the code and reproduced against a real
`sshd` through the installed GVfs on a private bus).** Better Files mounted a
server and then navigated to the folder GVfs shows it under,
`$XDG_RUNTIME_DIR/gvfs/sftp:host=…,port=…/path`, and pinning stored that path
as `file://…` in the bookmarks file. When the server stops, `gvfsd-sftp`
exits and the mount leaves GVfs's FUSE folder (`ls` gives ENOENT at once; an
SMB mount stays and gives EIO). The sidebar's `FilesystemProbe`
(`sidebar.rs`: `path.is_dir()` on the window's thread, for every favourite,
every frame, from `shell.rs` `nav_targets`) found nothing there, drew the row
"Unavailable", and `open_nav_target` (`shell.rs`) refused the click. A folder
open when its server went was reported `Gone` by the watch hub, and the tab
went to GVfs's own folder above it. Nothing remembered the address, and a
dead mount could freeze the window in that `is_dir()`.

**What was built.** `remote.rs`: the address model (from an address and from
a GVfs folder's name), the one rule `need`, the mount list read from GVfs's
tracker off the window's thread, the probe with a deadline, the attempts (one
per mount, joined), the reasons. The reader applies the rule to every listing
(`ListingError::NotConnected` / `NotAnswering`); `session/remote_tabs.rs`
connects the tab, joins, cancels, lands on the same folder, selection and
scroll, climbs to the nearest folder that is there, and retries a dropped
connection by itself. Favourites are stored as addresses and old ones
migrated; FileManager1 and the command line take server addresses; a paste
waits for the connection; recents on a server are listed without a `stat`;
the address bar's check and Tab completion do not read a server's folder.
Spec: `files-gui-policy.md`, "Remote folders connect when they are opened".

**Checks.** `remote_tests.rs` (fake connector) and `remote_servers_tests.rs`
(real `sshd` and `smbd` on loopback, GVfs on a private bus; with the server's
processes stopped, the slowest frame of the window's own work measured
1.5 ms over 15 s). Screenshots in `better-os-evidence/310/`, light and dark,
en and zh-TW, against the same servers in a headless sway. Not done here: the
owner's own servers and tailnet, a GNOME desktop, and CI's first run of the
real-server tests (its install step now names `openssh-server`, `samba`,
`gvfs-fuse`). A tailnet machine's SMB shares remain ticket 133.
