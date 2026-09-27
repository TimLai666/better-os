# Ticket 61 — Pressing Escape closes Better Launcher, and a test says so

**Epic:** Better Launcher (Issue #2) · **User Story:** a person can dismiss the
launcher overlay with Escape · **Branch:** `ticket-60-62` · **Blocked by:**
none · **Status:** released in v0.2.8

## What it delivers

Better Launcher has no titlebar by design, so Escape is the only way to close
it, and no test covers that key path. After this ticket the key-to-action
decision is a pure function outside GPUI and the overlay calls it, so the
Escape path is tested.

## Acceptance criteria

- [x] Escape maps to close; the other keys the overlay handles map to their
      current actions, with a test for each.
- [x] The overlay's behaviour is unchanged.

## What was built

`launcher_gui::model::key_action` is the key-to-action decision as a pure
function over GPUI's key name. It returns `KeyAction::Close` for `escape`,
`KeyAction::Launch` for `enter`, `KeyAction::Move` for the four arrows, `home`,
and `end`, and `None` for everything else, which the search row keeps. The
overlay's `on_key` now calls it and does exactly what it did before for each
answer. The table is the one the overlay's old `match` held, key for key, and
modifiers are still not consulted.

Four tests in `model.rs` cover it: Escape closes, Enter launches, each movement
key maps to its movement, and a list of other keys (letters, space, backspace,
tab, page keys, `Escape` with a capital, `esc`, the empty string) is left
alone.

Escape was also pressed on the built overlay, in a headless nested Wayland
compositor (sway, not GNOME): the overlay was on screen, Escape closed it, and
the process exited.

Found and not fixed: in that same session, Down moved the selection a row but
Right did not move it, and the unmodified `main` build behaves the same way.
The search row most likely takes Left and Right for its own cursor before the
overlay's capture listener sees them. That is outside this ticket and is
recorded here rather than changed.

Not verified: the step from `OverlayEvent::Closed` to `cx.quit()` in `main.rs`
has no automated test, because that needs a GPUI test context. Escape has still
not been pressed on a real GNOME desktop.
