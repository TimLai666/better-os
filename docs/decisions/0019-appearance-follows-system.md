# ADR 0019: Every window follows the system appearance by default

## Status

Accepted on 2026-11-14 by the owner (ticket 110). Supersedes
[ADR 0004](0004-dark-first-themeable-appearance.md).

## Decision

Every Better OS window opens in the desktop's light or dark appearance unless
the person picks one. The stored choice is still `system`, `light`, or `dark`,
and `system` is now the default everywhere: in `better_ui::fluent::theme::
Appearance`, in `manager-core`'s `StoredTheme`, and in each window's own
setting (Awake, Files, Git, Launcher, Manager, Monitor, Touchpad, the app
chooser, and the Fluent gallery).

- Nothing stored, a settings file written before the setting existed, and a
  value this version cannot read all mean `system`. An unreadable value never
  makes the rest of a settings file unreadable.
- `system` keeps following the desktop while a window is open, not only when
  it opens. On Linux GPUI learns the desktop's appearance from the XDG desktop
  portal in the background, after the first window exists, and reports light
  until then; a window that reads it once opens light on a dark desktop. Every
  window therefore applies the choice through `Appearance::follow` or
  `Appearance::apply_and_follow` and holds the subscription they return.
- A choice the person made (`light` or `dark`) is kept. Where a settings file
  has always written the appearance with every save (Awake, Git, Manager), a
  stored `dark` cannot be told from one written as the old default, so it is
  kept as dark. Where the appearance lives in its own one-word file, the file
  exists only once someone picked, so every stored value is a real choice.
- Every appearance picker lists the default first: "Use system setting"
  (跟隨系統), then Light, then Dark.

## Why

The owner decided on 2026-11-14 that every app follows the system appearance
by default. Under ADR 0004 a light desktop got dark Better OS windows and,
since ticket 108 draws the frame in the window's appearance, dark window
frames too.

## Consequences

- Someone who never picked an appearance and whose desktop is light sees
  Better OS turn light after updating. Someone whose Awake, Git, or Manager
  settings were saved at least once before keeps dark, because the file says
  dark; they can pick "Use system setting" once.
- Better Touchpad gains its own appearance choice (on its Overview page),
  stored in `$XDG_CONFIG_HOME/better-os/touchpad/appearance`.
