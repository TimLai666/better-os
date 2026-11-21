# Ticket 259 — A Better OS login screen laid out like Luma's lock screen

**Blocked by:** 203 (the session package) for running it; none for the design
**Blocked:** on the owner's answers to L1 to L8 at the top of `design/login-screen-spec.md` (L1, L2, L7 and L8 are ADR 0044's)

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

## Notes

Design delivered on 2026-11-21; the ticket stays open for the owner's
answers.

- **The spec**, `design/login-screen-spec.md`: the owner's decisions (L1 to L7, L8 added in the revision below)
  first, each with a recommendation and its costs; the sources read on the
  machine and upstream; Luma's screens measured from its source (section
  1); what Better OS follows and does not take; every state of the login
  screen and the lock screen with the zh-TW and English copy; how both are
  built (greetd's IPC, `cosmic-comp` or Cage, GPUI, the session lock
  protocol and a crash of the lock program, the Debian display-manager
  switch); installing, switching and removing.
- **The security review**, `engineering/specs/login-screen-security.md`, in
  the form of the touchpad pass-through review: accounts, reach, the
  password's path, crashes and hangs, ADR 0033's watch (it does not cover
  greetd), and the way back to GDM and into a locked session as numbered
  steps.
- **ADR 0044** (proposed): the lock screen now, the greeter as a choice in
  Better Manager later, not by default while GNOME is offered; switching on
  and off as one action each with GDM put back exactly.
- **Mockups**, `design/login-screen/`: nine views in light and dark, zh-TW,
  rendered in `/home/tim/Developer/better-os-evidence/259/`.
- **Tickets 260 to 269 and 278 to 281** (270 to 277 were already taken on another branch).

Findings that changed the design:

- Luma's desktop login and lock screen is GNOME's GDM and unlock dialog,
  restyled by GNOME Shell patches ("Poster"); its greetd greeter is the
  phone's. Its desktop login shows one person and no session menu.
- No image of Luma's lock or login screen is published (site, gallery, blog,
  repository), and "Try Luma" is a desktop application running a virtual
  machine, not a browser preview; it was not downloaded or run. The
  description is from source only; the `Try` page itself is captured in
  `/home/tim/Developer/better-os-evidence/259/reference/`.
- GNOME Shell 46 has no lock screen unless GDM runs (read from
  `js/misc/loginManager.js`), and greetd runs one session at a time: both
  are costs of replacing GDM, and are why ADR 0044 does not recommend the
  greeter by default.
- GPUI at the workspace's revision has no session lock window and no
  password content purpose (L6).

Revised on 2026-11-21 for the owner's two requirements:

- **Locking never depends on GDM.** The Better OS session locks through
  `cosmic-comp`'s own `ext_session_lock_v1` (confirmed in its source: while
  locked it draws only lock surfaces, a dead lock client leaves outputs
  black and the lock held, a new client may lock again). GNOME Shell 46
  needs only an answer on `org.gnome.DisplayManager` (`Version`,
  `OpenReauthenticationChannel` and a private `UserVerifier` connection),
  not GDM itself; ticket 280's `better-os-gnome-unlock` gives that answer
  without patching the shell (L2 (a)). Locking on another terminal fails
  the "never shows content" invariant; GDM cannot run beside another
  display manager. L1 now asks who owns the login and lock path.
- **The login screen always appears.** Spec section 5.10: every stage from
  the display manager's start to a prompt accepting input, each with a
  deadline, owned by a root login supervisor (ticket 267, rewritten from
  the greetd form of ADR 0033's watch); the property "within 60 seconds,
  the Better OS login screen or a text login is accepting input, Plymouth
  has quit, and the journal names the failed stage"; the fault-injection
  proof (ticket 281); the new decision L8 (no automatic GDM fall back).
  Found on the way: `gdm.service` quits Plymouth itself and only on failure
  does systemd, so an active, silent GDM leaves the splash up; the Linux
  console cannot draw Chinese, so the text fall back's message is English.
- ADR 0044 rewritten: Better OS owns the path on greetd (B), in six proven
  steps, the default left to a later decision. The security review covers
  the two new root services. The `session` mockup lost GNOME's "cannot
  lock" line; `text-fallback` is new.

Not done: nothing was run (no greetd, no `cosmic-comp` greeter, no VM); the
mockups' wallpaper is a gradient stand-in for ticket 279's artwork. The
Luma clone used for reading is in the session scratch directory, not under
`~/.cache/better-os/orch/luma-ref/`: the worktree guard refused a clone
there.
