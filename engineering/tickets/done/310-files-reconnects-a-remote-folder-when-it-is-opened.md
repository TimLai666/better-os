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
1.5 ms over 15 s). A tailnet machine's SMB shares remain ticket 133.

**Review round (2026-11-22, `better-os-evidence/310/review-2026-11-22.md`).**
Each item has its test in `remote.rs` or `remote_tests.rs` unless named.
- A mount is a favourite's only with the same identity both ways (scheme,
  host, port with the scheme's default filled in, user, share or domain):
  `a_mount_on_the_same_host_with_another_port_user_or_share_is_another_mount`.
- Only server kinds under GVfs's folder enter the rule; a phone, camera, Apple
  device, online account or unknown kind keeps the old availability, and only
  an address typed on the Address-needed page rewrites that favourite:
  `a_device_favourite_is_never_taken_for_a_server_s` (bookmarks file
  byte-identical),
  `only_an_address_typed_on_the_address_needed_page_rewrites_its_favourite`.
- A password in a typed address is dropped where it becomes a location:
  `a_password_in_an_address_never_reaches_the_location_or_the_spec`,
  `a_typed_password_reaches_none_of_the_four_places` (bookmark, history,
  restart hand-off, page; nor the mount request).
- A held paste goes in whichever way the wait ends, and says so when it
  cannot: `a_paste_held_while_a_folder_stopped_answering_goes_in_when_it_answers`,
  `a_held_paste_into_a_folder_deleted_meanwhile_is_not_done_and_says_so`
  (no folder made again), `a_held_cut_leaves_a_newer_clipboard_alone`.
- A requested unmount is told from a drop by looking at the host (GVfs's
  signals are identical, seen with `dbus-monitor` on the sandbox's bus);
  retries every 5 s, stop at 2 minutes and on another network:
  `a_mount_let_go_while_its_server_answers_is_not_mounted_again`,
  `retries_stop_at_their_limit_and_on_another_network`; a changed host key is
  never retried: `a_changed_host_key_after_a_drop_is_said_at_once_and_not_retried`.
- `sftp://nas/` stays the root: `a_mount_root_favourite_stays_the_root`,
  `a_mount_root_favourite_opens_the_root_not_the_home`; the
  bookmarks file is migrated once, only as read, keeping its mode:
  `the_bookmarks_file_is_migrated_once_and_only_as_it_was_read`.
- Recents GIO applications recorded as `sftp://` or `smb://` hrefs are not
  listed in Recent (only `file://` entries are, as before); the spec states
  the limit. Recents under GVfs's folder are listed without a `stat`.
- `gvfsd-smb` investigated against `smbd` (`remote_servers_tests.rs`, locally
  and in CI): unknown host "Invalid argument" (read as not found by the host
  look), stopped server "Connection refused", missing share "No such file or
  directory". The zh-TW screenshots then showed the share-gone sentence
  missing in Chinese, because the part after GVfs's own words is the C
  library's error text in the desktop's language; it is now read through
  `strerror_l` (`the_system_s_words_after_gvfs_s_are_read_in_chinese_too`).
- CI: both real-server tests ran and passed on the `rust` job of
  `eb66817f` (run 44940322126; FUSE and unprivileged `smbd` work on the
  runner), with the same SMB answers as here and a slowest frame of 0.9 ms.

**Evidence.** 60 screenshots in `better-os-evidence/310/`: 15 states (not
connected, connecting, cancelled, did not answer, opened, dropped and
reconnecting, back on the same selection, folder gone and the nearest above,
name not found, refused, share gone, wrong password asked again, sign-in
refused, address needed, no network) in light and dark, English and zh-TW,
from a sandbox with a fixture disk only, against loopback `sshd` and `smbd`
in a headless sway. The four Address-needed shots were taken again in the
second round: the page names the folder by GVfs's mount name
(`dav+sd:host=Cloud._webdav._tcp.local/data`), no runtime path or user id.
Not done here: the owner's own servers and tailnet, and a GNOME desktop.

**Second review round (2026-11-22, `review-2-2026-11-22.md`).** The retry
rule was decided by the coordinator and replaces the first round's "stop on
another network" and "a server that answers at first look let the mount go";
the spec has it once, under "A connection that drops while a folder is
open comes back by itself".
- No fact from GVfs tells a requested unmount from a drop: recorded with
  `dbus-monitor` on the sandbox's bus, `gio mount -u` and a stopped `sshd`
  both make the backend call `UnregisterMount` and the tracker send the same
  `Unmounted`; the unmount request goes over GVfs's private peer connection
  (after `GetConnection`). Better Files unmounts no server, so every drop
  reconnects: `a_mount_let_go_elsewhere_comes_back_by_itself`.
- No network (empty routes) is waited out with no limit and nothing sent;
  the same network back reconnects by itself; another network shows the
  new "another network" page with Connect. Networks: default routes,
  NetworkManager's primary profile UUID (system bus), gateway hardware
  address (`/proc/net/arp`); only an observed change is another network
  (the coordinator's decision after CI, below), and a server on this
  computer always comes back. Tests:
  `a_network_that_goes_and_comes_back_reconnects_by_itself`,
  `a_folder_comes_back_after_sleep_on_the_same_network`,
  `a_drop_onto_another_network_waits_for_a_click`,
  `a_dropped_connection_keeps_trying_until_the_server_is_back` (server
  restart), `retries_stop_at_their_limit`,
  `a_restarted_server_comes_back_when_the_network_cannot_be_read_more_closely`,
  `a_server_on_this_computer_comes_back_with_no_network`,
  `only_an_observed_change_is_another_network`,
  `a_gateway_s_hardware_address_is_read_from_the_neighbour_table`. The limit
  is now 10 minutes of time with a network (a NAS restart takes minutes).
- The host look no longer rewrites GVfs's failure; when it cannot resolve
  or reach, GVfs itself is asked every 30 s. `remote_servers_tests.rs` opens
  the SFTP server by an alias in the sandbox's own `ssh_config`: opened,
  dropped and back by itself, and its failure while away GVfs's
  "Connection refused by server". `smb://no-such-host.invalid` now shows
  "could not connect" with GVfs's "Invalid argument".
- Found by that test: a dropped connection past its first 30 s (its page
  showing why) was never taken back when a try succeeded, because landing
  and refusing only took links still "connecting". Fixed in `land` and
  `refuse`; the alias test fails without it.
- P3: `a_held_paste_whose_tab_goes_elsewhere_says_so_and_never_runs`,
  `a_paste_waiting_for_its_listing_is_said_not_done_when_the_connection_is_cancelled`
  (the waiting paste is set by a test helper; its timing is GVfs's),
  `an_address_typed_for_a_server_already_mounted_still_rewrites_the_favourite`,
  the empty user in `a_password_in_an_address_never_reaches_the_location_or_the_spec`;
  the bookmarks temporary file is created with the target's mode (read, not
  tested: the moment before the rename cannot be observed), and the rename
  race is stated in the spec.
- Mount names, seen in the sandbox with `gio mount`: `smb://…/DATA` and
  `smb://LOCALHOST…/Data` are mounted as `share=data`, `server=localhost`;
  `sftp://[::1]:p/` as `sftp:host=%3A%3A1,port=p`. The identity folds them
  the same way (`an_address_matches_the_mount_name_gvfs_folds_it_to`), and
  the real-server test opens a mounted share written in capitals and an
  `[::1]` address with no second mount.
- CI on `7967a7ba`: one of the two `rust` runs failed at "it came back by
  itself: None" (no wait left, so the tab had climbed above its folder; a
  network refusal would have shown `OtherNetwork`). Cause: right after the
  remount GVfs's FUSE folder can answer "not found" for a moment, and the
  tab opened the folder above at once. The climb now looks at the folder
  again every 0.5 s for up to 5 s first
  (`a_folder_that_is_not_found_for_a_moment_is_not_left`). The network rule
  was changed too, as decided: unreadable facts are no change. The
  real-server test prints the network reading before and after the drop;
  CI's runner reads `routes "enp39s0>0180000A", connection None,
  gateway_hardware Some(…)`, the same before and after.
- CI on `90a42a38`: one `rust` run failed one step earlier, "the favourite
  opens after the server is back: None", the same signature; not
  reproduced here in four whole-suite runs and a one-core run. Reading the
  paths that end with no wait and the folder not shown: a climb that found
  nothing (not even the mount's own folder) did nothing, leaving the tab on
  "not found" with nothing waiting; it now connects again
  (`a_climb_that_finds_nothing_connects_again`), and the re-check lasts
  5 s. Every message of the real-server test now says the wait, location,
  listing, notice and GVfs's folders, so another failure names its cause.
- Not seen: the "another network" page on screen (the sandbox cannot change
  the network), NetworkManager's UUID on a real Wi-Fi change.
