# Opening Better Launcher

Better Launcher can be opened three ways. Two of them work on every machine.
The third does not exist yet and is not required for the other two to work.

| Path | What runs | What it asks for | Available |
| --- | --- | --- | --- |
| Desktop entry | `better-launcher --open` | Open | Always |
| Global keyboard shortcut | `better-launcher` | Toggle | GNOME sessions, once the shortcut is applied |
| Five-finger pinch inward | a gesture adapter | Toggle | Never in this version — see [ADR 0008](decisions/0008-launcher-gesture-integration.md) |

A machine with no five-finger touchpad, or no GNOME session, is not in an error
state. It has a shorter list. `SessionCapabilities::activation_paths` is where
that list is produced, and the desktop entry and a second launch are always in
it.

## One overlay, however many times it is opened

The first `better-launcher` process takes the well-known session-bus name
`org.betteros.Launcher1` and serves `org.freedesktop.Application` at
`/org/betteros/Launcher1`. Every later launch is refused the name, hands its
request to the running process over that interface, and exits. So the overlay
is never drawn twice and the application index is never built twice.

The two verbs are kept apart on purpose:

- `Activate` — sent by the desktop entry, a dock, a panel, or `gio launch`.
  Opens. Never closes.
- `ActivateAction("toggle")` — sent by a second launch with no `--open`, which
  is what the keyboard shortcut runs. Opens a closed launcher and closes an
  open one.

This build's overlay is transient: the process lives while the overlay is on
screen, so a forwarded toggle ends the process. Whether the launcher should
instead stay resident is an open question, alongside the overlay's dimensions.

## The global keyboard shortcut

An unprivileged application on GNOME cannot register a system-wide shortcut for
itself, and Better OS does not grab the keyboard to fake one. What the launcher
does instead is name exactly which settings carry the shortcut. Applying them
is Better Defaults' job (ticket 27), over its own reviewed boundary.

GNOME stores each custom shortcut as one relocatable schema instance plus one
entry in a list:

| Setting | Value |
| --- | --- |
| `org.gnome.settings-daemon.plugins.media-keys custom-keybindings` | must contain the path below |
| `…/custom-keybindings/better-launcher/ name` | `Better Launcher` |
| `…/custom-keybindings/better-launcher/ command` | `better-launcher` |
| `…/custom-keybindings/better-launcher/ binding` | not decided |

The full path is
`/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/better-launcher/`,
and the relocatable schema is
`org.gnome.settings-daemon.plugins.media-keys.custom-keybinding`.

`command` is a program name with no arguments, no shell metacharacters, and no
interpolation — there is a test that says so. The binding itself is
deliberately unset: Issue #2 defers the exact key combination, and shipping a
default here would settle it silently. `launcher-platform::shortcut` is the one
place these strings exist, and the component manifest declares the same four
settings; a test compares the two so they cannot drift.

A user who wants the shortcut today can add it in Settings → Keyboard → Custom
Shortcuts with the command `better-launcher`.

## What is deliberately not here

- No compositor extension, no shell extension, and no GJS.
- No raw input device is opened and no input is grabbed.
- No process runs as root.
- No network request is made by any of it.
