# Ticket 172 — No Better OS screen shows a raw message key

**Blocked by:** none

## Why

Ticket 146 gave every Better Awake provider reason a sentence in both
languages. Its worker found the same defect one row over and left it: the
backend detail on Better Awake's Diagnostics page (`crates/awake-gui/src/
pages_records.rs`, `status.backend.detail`) still shows `awake.backend.*`
keys as they are. Ticket 131 found and fixed the same thing in the Touchpad
diagnostics. A key on screen is unreadable to the person it is shown to.

## What to build

- Every `awake.backend.*` key gets a sentence in English and zh-TW in the
  table `awake-ipc` already has for provider reasons, used by the Better Awake
  window and Better Manager's Keep awake page; an unknown key reads "Unknown
  reason (key)".
- Search every window for the same pattern (a dotted key, a Rust `Debug`
  rendering, or an error's raw text put on screen: Monitor, Files, Git,
  Manager, Launcher, the trays) and fix each at its source.
- A test per crate that every key its services can emit has both sentences,
  as `awake-ipc`'s does.

## Done when

- The tests above, and each fixed screen seen in zh-TW and English.

## Notes

In progress, not blocked: the audit below found more than one session could
fix, and no screen has been looked at yet (the "seen in zh-TW and English"
half of Done when).

### Done

- Better Awake: `awake_ipc::reasons` is one table for every key the service
  and the crates it is built from can send: `PROVIDER_REASONS`,
  `BACKEND_REASONS` (`not_probed`, `unavailable`, `denied`, `protocol`,
  `lease_lost`), and `SERVICE_REASONS` (sessions, rules, held-back rules,
  conflicts, requests, quick sessions, settings, stored files), English and
  zh-TW. `reason()` replaces `provider_reason()`: a detail that is a key is
  said as a second sentence, a row named for a detail
  (`invalid_quick_sessions:no_lengths`) is used for it, an unknown key reads
  "Unknown reason (key)" / 「原因不明（key）」. `awake_ipc::UNEXPECTED_REPLY`
  is the clients' own key for a reply of the wrong kind.
  The test scans awake-platform, awake-core, awake-service, awake-store, and
  awake-ipc for every `awake.*` key in production code and fails on one with
  no sentence in both languages, or a sentence with no key.
- Better Awake window: the backend detail (Status and Diagnostics),
  attention, a history entry's backend failure, the connection banner, the
  action banner, and the rule editor's refusals were keys; all are sentences
  now. `ClientError` is kept and said when drawn (`Copy::client_error`), with
  the system's own message after "The system said:" / 「系統回報：」.
- Better Manager's Keep awake page: the attention headline, the backend card,
  a refusal, and a failed read or change showed keys, and a reply of the
  wrong kind showed its `Debug` shape (`unexpected reply: {other:?}`); all are
  sentences now. An error report's details keep the system's own message.
- Better Touchpad: the overview's Health fact was `format!("{:?}")` of the
  state; it uses `health_words` like the Diagnostics rows.
- Better Files: a plugin command that failed reached the notice bar as the
  `Debug` shape of `RunError`; it is a `files.plugin.error.*` key with a
  sentence now. `files.prefs.error.unreadable` and
  `files.bookmarks.error.unwritable` had no sentence and were shown raw. A
  `files.`/`storage.` key `errors::explain` does not know now reads "Unknown
  reason (key)" instead of standing alone. `tests/errors.rs` covers all of it.
- Better Git: ticket 173's trash writer would have put a `files.trash.error`
  key in a discard's error; `git-platform` turns it into words about the file.

### Checked, nothing to fix

- Better Monitor: the link's `monitor.gui.link.*` details are never drawn
  (`failure_sentence`, `collection_*` sentences), and `{:?}` in the crate is
  only element IDs.
- Better Awake tray: menu labels show the session's own reason and a fixed
  attention label; refusals go through `failure_text`, which already says
  every key in plain words, and its labels test asserts no `awake.` key.
- Better Launcher: no key or `Debug` text reaches the overlay.
- Better Manager outside Keep awake: `evidence_label` maps every evidence key
  to a sentence, falling back to a generic refusal, never the key.
- Element IDs built with `{:?}` (files-gui `render.rs`, `panels.rs`,
  manager-gui `render_apps.rs`, `default_apps/render.rs`) and git-gui's graph
  hash are not on screen.

### Left (each is a raw text or key on a screen)

- Better Files: a sentence's `{detail}` can hold another layer's key, for
  example `OperationError::TrashUnavailable`'s reason is
  `files.trash.error.…` or `files.trash.shared.…` text, so "The trash can't be
  used here (files.trash.error.unusable:…)" is possible. The keys of
  files-platform (`files.trash.*`, `files.platform.*`), files-core
  (`files.listing.*`, `files.location.*`, `files.navigation.*`),
  files-preview, files-search, and storage-service (`storage.unknown.*`,
  `storage.writing.*`) need a scan test like awake-ipc's, deciding for each
  family whether it reaches `explain`.
- Better Git: `GitError::Io` and `GitError::Failed` show the system's or
  Git's English message as the message itself on a zh-TW screen
  (`git-gui/src/i18n.rs`, `GitError::Io(text) => text.clone()`).
- App chooser: `AssociationError`'s English sentences are an error report's
  details under a translated summary (`app-chooser-gui/src/chooser.rs`,
  `Notice::AssociationFailed`).
- Better Manager: `error_details` is `error_chain(error)`, the
  `platform.error.*`/`daemon.error.*` keys of the error chain, in the error
  report's details.
- No window above has been opened in English and zh-TW to look at the
  changed rows; that needs a sandbox desktop per window.
