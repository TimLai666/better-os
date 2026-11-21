# Ticket 259 — A Better OS login screen laid out like Luma's lock screen

**Blocked by:** 203 (the session package) for running it; none for the design

## Why

The owner asked on 2026-11-21 for a login screen with the layout of Luma's
lock and login screen (https://simplyluma.com, the Project Luma operating
system), written in Rust like the rest of Better OS. Luma's own greeter,
`luma-greeter` in `SimplyLuma/luma`, is C with GTK on greetd, so it cannot be
taken as it is (AGENTS.md: no C or GTK application code); its layout can be
built again. Luma's code is Apache-2.0 and its visual assets are CC BY-SA
4.0: the layout and behaviour may be followed, but none of its images, icons
or wallpapers are copied.

ADR 0038's C3 keeps GDM for now, because GDM lists both GNOME and the Better
OS session and a broken greeter would lock the person out of both. This
ticket therefore begins with a design and an ADR, not with replacing GDM.

## What to deliver first: a design

In the form of the project's other design tickets (a spec with every state
and the zh-TW and English copy, Fluent mockups in light and dark, owner
decisions at the top):

- The reference: what Luma's lock and login screen looks like and does, taken
  from its source (`src/luma-greeter/` and the desktop lock screen's source in
  `SimplyLuma/luma`) and from its browser preview, described in words and
  measured (the clock and date, the identity group of picture, name and entry,
  where each sits and how it moves between the quiet view and the entry
  view), with screenshots kept outside the repository.
- Better OS's screen: the same layout in the suite's own style, for a
  password (and what a PIN, a fingerprint or a smart card would need later),
  several accounts, a session choice between Better OS and GNOME, keyboard
  layout and input method at the prompt, accessibility, power and restart,
  Caps Lock and wrong-password states, a locked account, an expired password,
  several monitors and scale 2, and what shows while the session starts.
- One design for both the login screen and the lock screen inside the Better
  OS session, so the two look the same (ticket 201 found the session's lock
  program must be started inside the session scope).
- How it is built: a Rust program on greetd's IPC (PAM stays with greetd; the
  greeter holds no credential store), which compositor it runs in before
  anyone is logged in, how GPUI draws it there (ticket 200's surfaces; GPUI
  Kit and GPUI Fast after ticket 257), and what it must never do (no network,
  no reading of a home directory before authentication).
- The security review such a program needs before it replaces GDM: what runs
  as which account, what a crash leaves the person with, and the way back to
  GDM from a text console, written as steps.
- The ADR: when the Better OS greeter replaces GDM (never while GNOME is the
  default, per C3, unless the owner changes that), and how installing and
  removing it stays one action each with GDM put back exactly.
- The implementation tickets that follow.

## Done when

- The design, the mockups, the ADR and the follow-up tickets are in the
  repository, and the owner has answered the decisions the design lists.
