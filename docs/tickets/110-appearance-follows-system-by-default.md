# Ticket 110 — Every app follows the system appearance by default

**Blocked by:** none · **Status:** done

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

## What was built

- **Default.** `system` is the default in `better_ui::fluent::theme::Appearance`,
  `manager-core`'s `StoredTheme`, Awake's `StoredTheme`, Git's `Appearance`
  and its `Settings`, and the Fluent gallery's options. Files, Launcher,
  Monitor, the app chooser, the Files previewer, and Touchpad read the shared
  one-word file. ADR 0019 records the decision and supersedes ADR 0004;
  DESIGN.md, README.md, and delivery-status.md say so. AGENTS.md never said
  "dark-first", so it is unchanged.
- **Old settings files, per app.**
  - Files, Launcher, Monitor, app chooser, Touchpad: the word file is written
    only when someone picks, so a missing file is `system` and every stored
    word is a real choice and is kept.
  - Manager, Awake, Git: every save writes the appearance, so a stored `dark`
    written as the old default cannot be told apart from a chosen one. It is
    kept as dark. A file without the field loads as `system`.
  - An unreadable value is `system` everywhere. In Manager and Awake it used to
    make the whole file unreadable (Manager set its state aside as corrupt;
    Awake fell back to default presets); the theme field now reads leniently
    and the rest of the file is kept. Awake also now accepts a file without
    the field, which it used to reject whole.
- **Stored `system` not followed until toggled (owner report, 2026-11-14).**
  Reproduced in a headless GNOME Shell with `color-scheme` `prefer-dark` and a
  stored `system`: Awake and Git opened light, Files, Manager, and Monitor
  dark (`.shots/before-*-system-dark.png`). Cause: GPUI on Linux asks the XDG
  desktop portal for the color scheme on a background task
  (`gpui_linux` `xdg_desktop_portal.rs`) and reports light
  (`LinuxCommon::appearance` starts as `Light`) until the answer arrives,
  which is after the first window opened. The answer reaches a window as an
  appearance change. Awake (`Theme::sync_system_appearance` once) and Git
  (`theme::apply` read `window.appearance()` once; its comment claimed an
  observer in `render` that did not exist) never subscribed, so they kept
  the light they saw at startup; picking another choice and `system` again
  read the by-then-correct value. The Files previewer had the same one-shot
  `apply`. Touchpad and the gallery ignored the desktop altogether (fixed
  dark). Fix: `Appearance::follow` (paints now, and for `system` returns the
  subscription that repaints on every desktop change) and
  `apply_and_follow`, which every window now uses and keeps; Awake and
  Touchpad keep gpui-component's own palette through `follow`.
  After the fix all nine open dark (`.shots/after-*-system-dark.png`), and
  Awake, Git, and Touchpad follow a switch made while open
  (`.shots/switch-*`).
- **Pickers.** Every picker lists "Use system setting" / 「跟隨系統」 first,
  then Light, then Dark (Awake, Files, Git, Launcher, Manager, Monitor,
  Touchpad, gallery). Launcher and Monitor said 「跟隨系統設定」 and now match
  the others. Manager, Awake, and Monitor descriptions no longer say Better
  OS starts dark.
- **Touchpad** gains an Appearance card on its Overview page, stored in
  `$XDG_CONFIG_HOME/better-os/touchpad/appearance`, the directory its other
  settings already use.
- **Tests.** `better-ui`: a GPUI test that `follow` paints now and keeps a
  subscription only for `system`; load defaults. Per app: Awake
  (`the_stored_theme_defaults_to_the_system_and_keeps_a_choice`), Git
  (`tests/invocation.rs`), Manager (`manager-core` and `manager-gui`), Monitor,
  Launcher, Touchpad, and the gallery cover no file, a file without the
  field, an unreadable value, and a kept `dark` and `light`; Files and the
  app chooser call the same `Appearance::load` with nothing of their own and
  are covered by the `better-ui` test. GPUI exposes no
  public way to change a test window's appearance, so the repaint on a
  desktop switch is shown by the sandbox screenshots, not a unit test.
- **Screenshots** (`.shots/`, not committed): `nofile-light-*` and
  `nofile-dark-*` for all nine windows with no settings files,
  `stored-dark-on-light-better-touchpad.png` for a kept choice.
