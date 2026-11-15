# Ticket 117 — Better Files signs in to a server in the window

**Blocked by:** 93 (released) · **Status:** open

## Why

On 2026-11-14 the owner opened an `sftp://` address of a machine on their
tailnet in Better Files. Instead of a sign-in form, a warning banner showed
`gio mount`'s raw terminal dialogue: the host-key question ("the identity
sent by the remote computer is SHA256:… [1] Log in anyway [2] Cancel login,
Choice:") followed by "gio: sftp://…: the login dialog was cancelled", with
the long line running off the right edge of the window.

Cause: `crates/files-gui/src/network.rs` runs `gio mount <address>` and
writes answers to its standard input. It only knows the user/password
prompts, recognises them by English words ("password", "login",
`asks_for_password`, ~line 77) although `gio` answers in the desktop's
language, has no answer for the host-key question, and shows whatever `gio`
printed as the error.

## What to build

- Mount through GVfs's D-Bus interface with our own mount operation
  (`org.gtk.vfs.MountTracker` / `org.gtk.vfs.MountOperation`, the mechanism
  Nautilus uses through GIO), not by scripting `gio mount`'s terminal
  prompts. Every question GVfs asks arrives as a structured request:
  ask-password (which fields: user, domain, password, anonymous, remember),
  ask-question (message and choices), show-processes, aborted.
- Each request is shown in the window, in the content area where the folder
  would be (not a separate window), in the Fluent look:
  - Sign-in: the server's name, user name, domain only when asked for,
    password, "Remember" choices GVfs offers (forget at once, until logout,
    forever in the keyring), Connect and Cancel; Enter submits; a wrong
    password returns to the form with the reason and the user name kept.
  - Unknown or changed host key: what it means in plain words, the
    fingerprint in a monospace, wrapping, selectable line, and the choices
    GVfs offers (connect anyway, cancel). A changed key is worded as a
    warning, not as a first-time greeting.
  - Anonymous access when the server allows it.
- Errors are plain sentences in the app's language with the server's own
  detail underneath; long text wraps inside the banner and never runs off
  the window (check every banner in Better Files for the same overflow).
- Passwords are never logged, never put in process arguments, and are kept
  only by GVfs/the keyring when the person chose to remember.
- Tailscale: when `tailscale` is installed and running, the Network section
  lists the tailnet's online machines (from `tailscale status --json`, read
  only, off the UI thread) so they can be opened without typing an address;
  opening one tries `sftp://` with the tailnet name. When the peer accepts
  Tailscale SSH the connection needs no password and none is asked for;
  otherwise the sign-in form appears. Nothing is shown when Tailscale is
  absent or stopped.
- `gio` stays only as a fallback when GVfs's bus interface is unavailable,
  run with `LC_ALL=C` so its prompts can be recognised, and its raw output is
  never shown as the message.
- English and Traditional Chinese (Taiwan usage).

## Acceptance

- [x] Tests against a fake GVfs mount tracker on a private bus: password
      asked and accepted, wrong password then right one, domain asked,
      host-key question accepted and refused, anonymous, cancel, timeout,
      already mounted.
- [x] An end-to-end test or sandbox run against a real local `sshd` started
      by the test on a high port with its own host key and a throwaway
      account or key (no privileges, nothing outside the temp dir): first
      connection shows the host-key question, then the form, then the listing.
- [x] Tailscale listing tested with a fake `tailscale` on PATH: online and
      offline peers, not installed, not running.
- [x] Screenshots in a headless sandbox: sign-in form, wrong password,
      host-key question, changed host key, a long error wrapping, the
      Tailscale machines in the sidebar; both languages; light and dark.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **GVfs over its bus, not `gio`'s prompts** (`crates/files-gui/src/gvfs.rs`).
  A server address becomes the mount spec GIO's URI mappers build (`sftp`,
  `ftp`, `nfs` by host, port and user; `smb` by server and share, with
  `domain;user@`; `dav`/`davs` with `ssl`; `afp` by server and volume) and is
  mounted with `org.gtk.vfs.MountTracker.MountLocation` and a mount
  operation Better Files exports on its own connection. The signatures were
  read off the installed GVfs 1.54 (the tracker by introspecting `gvfsd` on a
  private bus; `org.gtk.vfs.MountOperation`, which GVfs only calls, from the
  method signatures in `libgvfscommon.so`) and confirmed by mounting a local
  `sshd` through a private `gvfsd`. The folder opened is the mount's folder
  under GVfs's FUSE mount (`LookupMount`), with the address's own path in it,
  or GVfs's default folder for the mount. "Already mounted" opens it.
- **Every question in the window.** `AskPassword` becomes the sign-in page
  in the content area: the server's name, the user, the domain only when
  asked, the password, the "Remember" choices GVfs offers (forget right
  away, until sign-out, in the keyring), anonymous access when the server
  allows it, Connect and Cancel; Enter signs in; a second request in the same
  mount is a wrong password: the page comes back saying so, with the user
  kept. GVfs's own words (which password it wants, a key's passphrase or the
  account's) are shown small under the title. `AskQuestion` for an SFTP host
  key it does not know shows what that means and the fingerprint
  (monospace, wrapping, selectable), with Connect anyway and Cancel; the
  key-against-address mismatch question is worded as a warning; any other
  question shows GVfs's words and its choices. Escape cancels; while a
  question is shown, keys that would act on the folder underneath do
  nothing. A cancelled mount says nothing.
- **Errors are sentences.** GVfs's GIO errors are read by their codes (not
  found, refused or unreachable, sign-in refused, timed out, not supported,
  cancelled) and said in the app's language with the server's own words as
  the detail under the sentence. A changed SSH host key (GVfs refuses it with
  code 0) is recognised by GVfs's "Host key verification failed" in English
  or in GVfs's own translation for the session (looked up in GVfs's catalog
  with the session's locale on that thread only) and explained as a warning:
  what it may mean and how to clear an expected change.
- **Long text wraps.** The kit's `InfoBar` title could not shrink below its
  longest line and ran off the window; it now wraps inside the bar (the
  message too), which fixes every banner in Better Files and the other
  apps. The sign-in page's texts wrap as well.
- **No password is kept.** Passwords travel in the window's `Secret` type
  (overwritten when dropped, never cloned or printed), go to GVfs over the
  bus only, and the field is emptied with its undo history when the form is
  sent or cancelled. Only GVfs and the keyring keep one, when the person
  chose to remember it. No password is logged or put in a process's
  arguments.
- **A mount that hears nothing for 30 seconds** (no question, no answer) is
  given up on; the time a person spends at the form never counts.
- **`gio` only as a fallback** when GVfs is not on the session bus: run in
  the C locale, its questions asked through the same pages, run again with
  the answers on its standard input; its output is only ever the detail
  under a sentence.
- **Tailscale.** The Network section lists the tailnet's online machines
  from `tailscale status --json` (read only, off the window's thread, every
  minute), each with its tailnet name; opening one tries `sftp://` to that
  name, so a peer that accepts Tailscale SSH opens without a password and any
  other shows the sign-in page. Nothing is shown when Tailscale is not
  installed, stopped or logged out.

Tests: `gvfs.rs` (address to mount spec for every scheme, GIO error codes);
`gvfs_bus_tests.rs` runs the real mounter against a fake GVfs mount tracker
on a private `dbus-daemon`: a password asked and accepted (with "remember
for the session" sent as 1), a wrong password then the right one, a share
asking for a domain and used anonymously, a new host key accepted and
refused, a changed host key, a cancelled sign-in, a server that never
answers timing out while a person slow to type does not, and an address
already mounted; `tests.rs` drives the window's session through a wrong
then right password and a cancel; `tests/share.rs` runs a fake `tailscale`
for online and offline peers, not installed, stopped and logged out.

End to end, in a headless GNOME Shell sandbox with its own bus and `gvfsd`,
against an unprivileged `sshd` on 127.0.0.1:22917 with a throwaway host key
and a passphrase-protected throwaway key (all in a temp dir; an `ssh`
wrapper first on `PATH` gives `ssh` that dir's config and known-hosts file,
and a `HostKeyAlias`, since GVfs passes
`-oNoHostAuthenticationForLocalhost yes`): the first connection shows the
host-key question, then the sign-in form (the key's passphrase; sshd cannot
check account passwords without privileges), a wrong passphrase brings the
form back saying so, the right one lists the server's folder; with the
recorded key changed the warning banner wraps. Screenshots in `.shots/`:
`t117-host-key-*`, `t117-sign-in-*`, `t117-wrong-password-*`,
`t117-listing-*`, `t117-key-changed-*` (the long warning wrapping), each
`en`/`zh` and `light`/`dark`; every one shows the fixture tailnet's two
online machines in the sidebar (the offline one left out).

## After review

- **Only GVfs can ask.** The mount operation is exported at a path with 128
  random bits, and every method checks its caller: GVfs's backends call the
  client's operation themselves on their own session-bus connection
  (`common/gmountsource.c` makes the proxy with `G_BUS_TYPE_SESSION` in the
  backend's process; seen on the lab bus as `gvfsd-sftp`, a child of the
  `gvfsd` that owns `org.gtk.vfs.Daemon`), so the caller's process (from
  `GetConnectionUnixProcessID`) must be that `gvfsd` or below it. Anyone
  else gets an aborted answer and the person sees nothing.
- **Chinese reads as Chinese.** GVfs's catalog is read with the session's
  messages and character type and bound to UTF-8, so its translated "Host
  key verification failed" matches and a changed key is shown as the warning
  in a Chinese session too (before, it came back as question marks).
- **Choices by GVfs's own order.** Connect anyway is GVfs's choice 0, which
  its SFTP backend answers ssh "yes" for (`gvfsbackendsftp.c`,
  `login_answer_yes_no`); the `gio` fallback reads the numbers `gio` prints
  (`[1] Log In Anyway`) instead of assuming one, asks only an SFTP host key
  (in Better Files' words), and never shows `gio`'s text as the question.
- **Passwords.** The `gio` fallback keeps the password in a `Secret` until it
  writes it; the bus reply is the only other copy. Closing the window
  (the compositor's close, Alt+F4) empties the password fields and cancels
  a sign-in on screen.
- **After 30 silent seconds** the location is looked up once more and opened
  if it did mount after all; a question GVfs asks later is answered
  "aborted" for two minutes rather than "no such object". When GVfs
  withdraws a question (`Aborted`), it leaves the screen.

Tests added: another program asking for the password during a mount is
refused and nothing reaches the window (`dbus-send` in a session of its
own); a mount that finished unannounced opens after the wait; a late
question is answered aborted; a withdrawn question closes on the window's
side; Connect anyway is checked against the fake's GVfs-order choices;
GVfs's zh_TW "Host key verification failed" read as UTF-8 (needs the
zh_TW.UTF-8 locale and Ubuntu's language pack, otherwise it says it is
skipped); a Chinese host-key question; `gio`'s numbered choices; the
process-ancestry check. The fake GVfs's silence limit is 3 s so a loaded CI
runner's fake still answers in time.
- **GVfs's first question is never lost.** zbus starts a connection's
  object server lazily, so a question GVfs asked the moment the mount began
  could reach a server not yet listening and hang until the mount was given
  up on (seen once in about eight runs under load). The mount connection
  now serves an empty object from the start, so the server listens before
  the first call; 25 runs under the same load: no failure. Two windows
  reading one drive's free space at once also share the one read instead of
  the second showing no bar.
