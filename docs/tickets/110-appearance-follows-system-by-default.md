# Ticket 110 — Every app follows the system appearance by default

**Blocked by:** none · **Status:** open

## Why

The owner decided on 2026-11-14 that every Better OS app opens in the
desktop's light or dark appearance unless the person picks one, replacing
ADR 0004's dark-first default. Today a missing or unreadable choice is dark in
`better_ui::fluent::Appearance` (`crates/better-ui/src/fluent/theme.rs`),
`manager-core`'s `StoredTheme`, and each app's own settings, so a light desktop
gets dark Better OS windows (and, since ticket 108, dark window frames).

## What to build

- The default appearance is `system` everywhere: `Appearance::default()`,
  every app's stored setting type, a settings file that predates the setting,
  and an unreadable value. Apps: Awake, Files, Git, Launcher, Manager, Monitor,
  Touchpad, the app chooser and the Fluent gallery.
- `system` keeps following the desktop while a window is open (it already
  does where implemented; make every app do it).
- A value the person chose explicitly (`dark` or `light`) is kept.
- Settings files where the stored `dark` was only ever written as the default
  cannot be told apart from a chosen `dark`. Find out per app whether the file
  records that the value was chosen (for example the setting is absent until
  changed); if it can be told apart, treat the unchosen one as `system`. If it
  cannot, keep the stored value and say so in What was built.
- Each app's appearance picker shows the new default first or marked as
  default, in English and Traditional Chinese, consistent with the other
  apps.
- ADR 0004 is superseded by a new ADR recording the owner's decision; DESIGN.md
  and AGENTS.md stop saying "dark-first" where they describe the default.

## Acceptance

- [ ] Unit tests per app: no settings file, an old file without the field, and
      an unreadable value all load as `system`; a stored `dark` or `light`
      stays.
- [ ] Screenshots in a headless sandbox of each app under a light and a dark
      desktop with no settings file, showing the app and its frame following
      the desktop.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
