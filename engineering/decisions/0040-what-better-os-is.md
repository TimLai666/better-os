# ADR 0040: What Better OS is, now that it has a session of its own

## Status

Proposed on 2026-11-21, for the owner to accept or refuse. It changes what
`AGENTS.md` says the project is, which is why it is a record and not an edit.
The owner's choice of option D in
[ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md) is decided
and already noted in `AGENTS.md` by one dated sentence; this record proposes
the rest of the wording, for acceptance before the session becomes the default
(spec `engineering/specs/better-os-session.md`, "Ready to become the default",
item 7; ticket 228).

## Context

`AGENTS.md` opens: "Better OS is a modular performance-improvement layer for
Zorin OS and Ubuntu. It is not a Linux distribution fork." Under option D
Better OS also ships a desktop session: its own launcher, panels,
notifications, and desktop on `cosmic-comp`, a patched compositor package
(ADR 0039, proposed), and a third-party package source on the person's machine
(C2). It is still not a distribution: it installs on Zorin OS and Ubuntu 24.04
through Better Manager and removes cleanly. Ticket 76's direction 3, a
Debian-based distribution, stays the long-term goal and needs its own record.

Three parts of `AGENTS.md` are written as if GNOME were the only desktop:

1. the opening statement;
2. the GJS paragraph, which grants a bounded exception for two GNOME Shell
   extensions; the session uses no GJS, and GNOME stays supported;
3. the follow-ups that describe GNOME 46 limits as Better OS's limits (the
   Mac-style preset rows that do not work on GNOME 46, "nothing animates", the
   GNOME Shell adapter run once, the static `GNOME_46_GESTURES` model, the
   GNOME-only custom-shortcut collision check, Better Defaults' dconf write
   path, the frame rule built on `better_ui::system_frame`).

## Options

### W1. The opening statement

- **(a)** "Better OS is a modular performance-improvement layer for Zorin OS
  and Ubuntu, with a desktop session of its own on COSMIC's compositor beside
  GNOME. It is not a Linux distribution fork. First-party components,
  the session included, are installed, updated, verified, and rolled back
  through shared manager operations."
- **(b)** Leave the statement and add the session as a follow-up. A reader
  learns what the project is from the first paragraph, so the session would be
  missing from it.

### W2. The GJS paragraph while GNOME stays supported

- **(a)** Keep it word for word, and add one sentence: "The exception covers
  the GNOME session only; no GJS, and no code of any language running inside a
  compositor, is part of the Better OS session, apart from the compositor patch
  ADR 0039 governs." The compositor patch is Rust, governed by its own ADR,
  and is not shell code.
- **(b)** Mark the paragraph as ending when GNOME support ends. Premature:
  GNOME is the default until ticket 228.

### W3. The GNOME-only follow-ups

- **(a)** Move each into one follow-up headed "GNOME 46 session limits",
  unchanged in content, and add beside each the session's state in one line
  (for example: "In the Better OS session the four-finger workspace switch
  follows the fingers with the released compositor; every other system gesture
  needs ticket 223's compositor").
- **(b)** Leave them where they are. They then read as limits of Better OS as
  a whole, which stops being true as the session's tickets land.

## Recommendation

**W1 (a), W2 (a), W3 (a)**, applied in one change when the owner accepts, with
the follow-ups' session lines filled in from the tickets done by then.

## Consequences

- `AGENTS.md` describes two desktops; every rule that names one (the frame
  rule, the user-unit rule's GNOME extension handling, the sandbox rule's
  headless GNOME Shell) says which.
- Making the session the default (ticket 228) is then a change of default,
  not of what the project is.
