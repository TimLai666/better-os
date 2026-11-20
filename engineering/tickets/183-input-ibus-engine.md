# Ticket 183 — The IBus engine process

**Blocked by:** 180

## Why

`input-core` decides everything; something has to speak IBus to GNOME. Spec
sections 3.1, 6.5, and 7.6 say how the engine draws the composition, what GNOME
shows of it, and how the engine runs the new program after an update.

## What to build

- **First, in a headless GNOME Shell 46 (AGENTS.md's sandbox rules) with its
  own `ibus-daemon`**: whether an engine process that exits is started again by
  `ibus-daemon` on the next focus by itself, or only after `SetGlobalEngine`
  (spec section 6.5); and which version of `zwp_text_input_manager_v3` Mutter
  46 advertises (styled preedit or not). Record both in this ticket's Notes.
- **`better-input-engine`** (`/usr/libexec/better-input/engine --ibus`): the
  IBus engine protocol on `zbus` (connecting to the address `ibus-daemon`
  publishes), `ProcessKeyEvent` with press and release, focus in and out,
  `Reset`, `SetSurroundingText`, `SetContentType` (password and PIN), cursor
  location; preedit through `UpdatePreeditTextWithMode` in commit mode, cleared
  for an unfinished syllable (spec section 3.1); the candidate table one page at
  a time with the person's page size and labels; the auxiliary text for Tab
  suggestions and the listening state; the `InputMode` property with symbols 中,
  英, 聽 (spec section 7.6); calls handled in arrival order.
- **The component file** `/usr/share/ibus/component/better-input.xml`: engine
  `better-input`, language `zh_TW`, layout `us`, symbol 中, setup
  `better-manager --page input`.
- **Update** (spec section 6.5, decision D9): the engine watches its executable
  and exits when it is replaced and no context has a composition; the AGENTS.md
  amendment adding this fourth mechanism (with the owner's answer to D9) lands in
  the same change.
- **Session-bus side**: owns `org.betteros.Input1.Engine` for the voice service
  (ticket 187 fills it), reached only by `better-input-voice` checked by its
  executable path.

## Done when

- [ ] The two facts are in Notes, with how they were seen.
- [ ] Protocol tests against a fake `ibus-daemon` on a private bus: key events,
      preedit mode and the unfinished-syllable rule, candidates and paging,
      auxiliary text, properties, password content type, call ordering.
- [ ] In the headless shell, installed into the sandbox's own root: typing into
      a GTK 4 test window produces the expected commits; focus change commits the
      preedit once; replacing the executable makes the old process exit when idle
      and the next key is handled by the new one. Screenshots in
      `/home/tim/Developer/better-os-evidence/183/`.
- [ ] fmt, clippy on stable, the crates' tests pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

This ticket is GNOME's: IBus serves `cosmic-comp` no Wayland input method (ADR 0038), and in the session the input method is Fcitx 5 with Chewing (C4). Better Input reaches applications in the session through ticket 221's front end, chosen by the owner's decision S4 in `engineering/specs/better-os-session.md`; this ticket is unchanged for GNOME.
