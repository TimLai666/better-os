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

## Notes

- One rule: `better_core::desktop::CurrentDesktop` (`crates/better-core/src/desktop.rs`).
  A session matches a declaration when any `XDG_CURRENT_DESKTOP` entry is one
  it names, ignoring case; `DESKTOP_SESSION` is read only when that list is
  empty. `DefaultIntegration::applies_to` takes a `CurrentDesktop`, and
  `defaults_core::adapters::desktop_session()` records every entry
  (`zorin:gnome`) instead of the first.
- Folded into the same rule: the touchpad GNOME check
  (`touchpad_platform::session::Session::is_gnome`), the launcher's shell
  detection (`launcher_platform::session::SessionCapabilities`), and the About
  line (`manager-gui` `pages/system/decide.rs::desktop_line`).
- Different questions, left as they are: `app-catalog-core`'s
  `DesktopEnvironments` and `monitor-gui`'s autostart `for_desktops` match a
  desktop entry's `OnlyShowIn`/`NotShowIn`, which the desktop-entry
  specification compares against `XDG_CURRENT_DESKTOP` alone (no
  `DESKTOP_SESSION` fallback); `monitor-service`'s inventory reports the
  variable verbatim as a fact about the session, and decides nothing with it.
- Tests: `zorin:GNOME`, `ubuntu:GNOME`, `GNOME`, empty, `KDE`, and the
  fallback, in `better-core` (unit and `tests/default_integrations.rs`),
  `touchpad-platform`, `launcher-platform`, and a `manager-cli` end-to-end
  `defaults inspect` under `zorin:GNOME` and `ubuntu:GNOME`.
- Seen on the owner's Zorin 18 machine (2026-11-21), session
  `XDG_CURRENT_DESKTOP=zorin:GNOME`, `DESKTOP_SESSION=zorin`: the worktree's
  `better-manager-cli defaults inspect` (debug build, never installed; state
  and catalog copied to a scratch path, `BETTER_OS_TEST_SYSTEMCTL_GUARD` set so
  the startup pass ran no `systemctl` or `gsettings`) now reads the real
  defaults instead of `defaults.not_supported_on_this_system`:

  ```
  better-files partially-default
    default-file-manager ApplicationHandler not-default current=Unset desired=DesktopEntry("io.betteros.Files.desktop") ...
    file-manager-tool-entry-point ToolEntryPoint unavailable (gsettings.no_such_key) ...
    overview-entry DesktopLauncherEntry default current=Set { value: DesktopEntry("io.betteros.Files.desktop") } ...
  ```

  The Default apps page gets its session from the same
  `defaults_core::adapters::desktop_session()`
  (`crates/manager-gui/src/pages/default_apps/jobs.rs`); the window itself was
  not opened on this machine, because that needs the build installed.
