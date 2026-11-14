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

- [ ] Tests against a fake GVfs mount tracker on a private bus: password
      asked and accepted, wrong password then right one, domain asked,
      host-key question accepted and refused, anonymous, cancel, timeout,
      already mounted.
- [ ] An end-to-end test or sandbox run against a real local `sshd` started
      by the test on a high port with its own host key and a throwaway
      account or key (no privileges, nothing outside the temp dir): first
      connection shows the host-key question, then the form, then the listing.
- [ ] Tailscale listing tested with a fake `tailscale` on PATH: online and
      offline peers, not installed, not running.
- [ ] Screenshots in a headless sandbox: sign-in form, wrong password,
      host-key question, changed host key, a long error wrapping, the
      Tailscale machines in the sidebar; both languages; light and dark.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
