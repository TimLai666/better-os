# Ticket 301 — One "show this file in the file manager" call

**Spec:** AGENTS.md ("A rule that two places need lives in one place both call")
**Blocked by:** none

## Why

Two crates ask the person's file manager to show a file over
`org.freedesktop.FileManager1`, each with its own copy of the call:

- Better Git: `crates/git-gui/src/file_manager.rs` (`show`,
  `show_in_session`, `show_unless_guarded`): `ShowFolders` for a folder,
  `ShowItems` for a file, refused under the test guard
  (`better_core::test_guard`) so a test never starts an installed file
  manager;
- Better Screenshot: `crates/screenshot-gui/src/service.rs`
  (`show_in_files`): `ShowItems`, falling back to the folder through the
  portal's `OpenDirectory` with the file's descriptor when no file manager
  answers (the portal refuses a `file:` URI to `OpenURI`). It does not
  check the test guard.

The two already differ (the guard, the fallback), and Better Files' own
windows and any later "Show in Files" button would make a third.

## What to build

- One function in a no-GPUI crate both can use (`better-core`, beside
  `better_core::uri::file_uri`, which both already use for the URI, or a
  small crate if `better-core` must stay free of zbus): the method by
  whether the path is a folder, the call on a given connection, the test
  guard, and the `OpenDirectory` fallback, with its tests moved from
  `git-gui/src/file_manager.rs`.
- Better Git and Better Screenshot call it.

## Done when

- [ ] One implementation; both crates call it; no other code in the
      workspace calls `ShowItems` or `ShowFolders` as a client (a test
      greps for it).
- [ ] The tests that were in `git-gui` pass from the shared place; fmt,
      clippy, and the crates' tests pass.
