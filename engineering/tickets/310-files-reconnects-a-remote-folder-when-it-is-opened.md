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

- [ ] The spec states the rule, the stored form of a remote favourite, every
      state and its sentences in zh-TW and English.
- [ ] Tests on the GPUI-free model for every entry point above, the join of a
      second attempt, cancel, each failure reason, the migration of old
      favourites, and the deadline; against a fake connector.
- [ ] A check against real servers in the sandbox: an SFTP server and an SMB
      server started by the test itself (in the brief's sandbox, on loopback,
      with fixture accounts), a folder pinned, the server stopped and started
      again, and the favourite opening the folder with no further action;
      wrong password, server away, and folder deleted each showing their
      page. Screenshots light and dark, zh-TW and English, under
      `better-os-evidence/310/`.
- [ ] The sidebar and the window stay responsive with a favourite whose
      server silently drops packets (a firewalled loopback port or a stopped
      process holding the socket), measured.
