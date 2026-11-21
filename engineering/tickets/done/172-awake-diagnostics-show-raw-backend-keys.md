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
- Better Launcher: wrongly listed here in the first round; its launch
  failure's details carried a key (fixed below).
- Better Manager outside Keep awake: `evidence_label` maps every evidence key
  to a sentence, falling back to a generic refusal, never the key.
- Element IDs built with `{:?}` (files-gui `render.rs`, `panels.rs`,
  manager-gui `render_apps.rs`, `default_apps/render.rs`) and git-gui's graph
  hash are not on screen.

### Finished after review (2026-11-21)

Every place the first round left, and what the review found beyond it:

- Better Files: `errors::SENTENCES` (English and zh-TW) holds every message
  key files-core, files-platform, files-operations, files-preview,
  files-search, storage-core, storage-service, and the window can send:
  listing, location, tab, platform, trash (`error` and `shared`), archive
  entry and archive error, saved-job, hard-link, preview, device-state
  (`storage.unknown`, `storage.writing`, `storage.performance`), and
  application warnings. A sentence's `{detail}` that carries another layer's
  key is not shown as the key: the part before it fills the slot and the key
  is said as a second sentence ("The trash can't be used here, so nothing was
  replaced. There is no home folder to keep the trash in."). Which archive
  limit was reached is said in words with its number. A tab that is gone was
  shown as `files.navigation.error.no_such_tab:…`; it is a sentence.
  `tests/errors.rs` scans those crates' production code: every key has a
  sentence in both languages, none shows a key or an empty "()", and every
  sentence is emitted by something (the table itself is not scanned). The
  enum-name families the window labels from their typed value
  (`files.job.state`, `files.operation.kind`, `files.conflict.kind`,
  `files.job.confidence`, `files.archive.format`, `files.search.scope`,
  `files.view`, `files.scale`) are listed in the test with that reason.
- Better Git: a failure Better Git has no sentence for says so ("Git stopped
  with a message Better Git doesn't recognize. What Git said is in the
  details."), an I/O failure says a file could not be read or written, and a
  discard's trash failure and a submodule refusal have sentences of their
  own (ticket 173). Git's or the system's text is only the detail, under
  "What Git said:" / 「Git 的原文：」 or "What the system said:" /
  「系統的原文：」; no `git.error` key is in the details either.
- App chooser: a failed launch's details are the catalog's sentence
  (`app_catalog_core::reasons`, a new table for every `catalog.error`,
  `catalog.launch.error`, and `catalog.platform.error` key, with its own scan
  test), and a default that could not be written or put back is said per
  failure in the window's language, with the system's words under a label,
  instead of `AssociationError`'s English sentences.
- Better Manager: an error report's details (`error_chain`) are said line by
  line from `manager-gui/src/reasons.rs`, a table for every
  `manager.error`, `manager.state`, `platform.error`, `daemon.error`,
  `ipc.error`, and `catalog.fetch` key the manager, platform, IPC, daemon,
  store, hardware, and drive crates can send (scan test, both directions).
  A key inside a key is a second sentence, a sentence with no place for its
  detail leaves it out (so `not_cancelable:{stage:?}`'s debug shape is never
  drawn), and the system's words keep their label. The System page's own
  English details ("the system bus is not reachable") are keys with
  sentences now; the drive and lighting rows use the same table.
- Better Touchpad standalone window: the gestures page showed the problem
  key (`gestures.export.no_path`, `gestures.import.no_such_file:…`), the
  editor's refusal, and a shortcut check's reason as keys; all go through
  `touchpad_views::i18n::problem_sentence` now, with `problem_detail` giving
  what follows the key (Better Manager's Touchpad page uses it for its error
  report too, instead of the raw problem). Every `gestures.*` key the touchpad
  crates send has its own sentence, not the catch-all
  (`touchpad-views/tests/problem_sentences.rs`). A damaged gesture file read
  "{path} could not be read as a gesture configuration: gestures.config.…",
  so the key was not found at all; it reads "gestures.config.…: … ({path})".
- Better Launcher (the first round's notes were wrong to say Launcher shows
  no key): a failed launch's error report detail was
  `launcher.platform.error.unknown_application:<id>`; it is a sentence with
  the id in parentheses, a catalog key is the catalog's sentence, and other
  text is the system's under a label (`i18n::failure_details`, tested).
- Better Awake banner: it repeated its title and ran two sentences together;
  the title says the service is not running, the line under it gives only
  the system's words, and every other failure is one punctuated statement.
  A call to a service nobody runs was also classified as "the connection
  failed"; `client::call_error` reads the bus's ServiceUnknown/NameHasNoOwner
  as "not running".
- Awake numbers: a number goes into the sentence with what it is ("A length
  of 0 minutes…", 「共有 9 個長度…」, "The battery is at 12%…"), or is left
  out; it is never hung on the end.
- Awake scan test: the table file is no longer read as "emitted", so the
  reverse check can fail, and it did: `awake.provider.no_power_supply` was
  only ever written by awake-core's tests; its row is gone.

### Seen (screenshots)

In a headless GNOME Shell 46 sandbox (a `systemd-run --user --scope` with
`MemoryMax=4G`, a no-op `ibus-daemon` and a fake `tailscale` first on
`PATH`, a private session bus whose only service directory holds
`ca.desrt.dconf`, `busctl --user list` showing no Better OS service), each in
English and zh-TW, under `/home/tim/Developer/better-os-evidence/172/`:
the Better Awake window with no service, Better Manager's Keep awake page
with no service and its details open, Better Git on a scratch repository
whose index is damaged with the details open, the standalone Touchpad
window with a damaged gesture file and with an import of a missing file,
and Better Files with a damaged settings file (cropped to the toolbar and
notice, so the machine's own drives are not in the picture).

Not produced in the sandbox, so the tests carry them: a missing plugin in
Better Files (typing `plugin:none` into the path bar through the remote
keyboard ended in "That location does not exist", so the plugin branch was
not reached that way; the sentence is in `tests/errors.rs`), a Better Files
plugin that fails, Better Launcher's failed launch, the app chooser's
failures, Better Manager's main error report, and a discard's trash failure
in Better Git.

### Checked: the original text as a detail

These show no key and no debug shape. Their headline is a sentence in the
window's language and the original English text is the error report's
detail, which is the pattern this ticket settled on (GitHub Desktop's and
Better Git's "What Git said"); the text is the crate's own English rather
than the system's, so each could be given sentences later:

- `touchpad-core`'s `StoreError` under a Touchpad gesture problem (the
  headline is "That didn't work.").
- `window-attention-core`'s store errors under Better Manager's Window
  attention error report.
- `drive-mounts`' `SetOutcome::Failed` for a D-Bus error nobody classified
  ("{name}: {message}"), which Better Files shows in its notice.
