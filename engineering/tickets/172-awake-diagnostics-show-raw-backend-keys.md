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
