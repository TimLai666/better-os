# Ticket 134 — Default apps are "unavailable" on Zorin because the session reads as "zorin"

**Blocked by:** none

## Why

The owner's Default apps page (0.2.16) shows Better Files and all three of
its settings as 無法使用 / 無法讀取, never checked.
`better-manager-cli defaults inspect` on that machine reports every
integration as `defaults.not_supported_on_this_system`.

## Cause (found 2026-11-16)

- Zorin sets `XDG_CURRENT_DESKTOP=zorin:GNOME`.
- `defaults_core::adapters::desktop_session` (`crates/defaults-core/src/adapters.rs`)
  takes only the first colon-separated entry, so the session is `zorin`.
- Every Better Files integration declares `sessions: [gnome]`, so
  `DefaultIntegration::applies_to` refuses all of them and the engine never
  reads the current value.
- `XDG_CURRENT_DESKTOP` is a list by the XDG spec; Zorin's, Ubuntu's
  (`ubuntu:GNOME`) and Pop's put their own name first.

## What to build

- The session matches a declaration when any entry of `XDG_CURRENT_DESKTOP`
  (case-insensitive) is one it names; `DESKTOP_SESSION` is only a fallback.
  One function decides it, used by every caller (check for other copies of
  this reading, e.g. in touchpad or window-attention code).
- Tests: `zorin:GNOME`, `ubuntu:GNOME`, `GNOME`, empty, `KDE`.
- See it on the owner's machine: the page reads the real defaults.
