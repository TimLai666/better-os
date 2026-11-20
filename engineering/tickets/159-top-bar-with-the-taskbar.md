# Ticket 159 — The top bar with the bottom taskbar, and on the owner's desktop

**Blocked by:** 152, 156, 157, 158, 149

**Blocked:** waits, by ADR 0038 (owner, 2026-11-21: option D). ADR 0038's
table puts this ticket on **wait**: its Better Defaults entry edits Zorin's
taskbar, which the Better OS session does not have, and its desktop check is
of the GNOME bar, which waits with ticket 151. The session's taskbar is ticket
213 and its bar is checked on the owner's machine in ticket 227. This ticket
is built when ticket 151 is.

## Why

`design/top-bar-spec.md` section 10: while the bar is on, the bottom taskbar
must not repeat what the bar shows, the two must never overlap, and turning the
bar off must leave the taskbar exactly as it was. The design also has to be
seen once on the owner's own desktop, at scale 2, with the person's own
taskbar. The owner chose D3 (a) on 2026-11-21: Zorin's taskbar stays, minus
its clock and system menu, until ticket 142's Better OS taskbar replaces it
(D3 (c), built in ticket 142, not here).

## What to build

- **Decision D3 (a)**: a Better Defaults entry "Taskbar without clock and
  system menu" that sets `visible: false` on `dateMenu` and `systemMenu` in
  every monitor's entry of
  `org.gnome.shell.extensions.zorin-taskbar panel-element-positions`, keeping
  every other element and the JSON's order, captured and restored exactly;
  "nothing to do" without Zorin's taskbar. On Ubuntu, Ubuntu Dock has neither,
  so nothing is written.
- **Geometry checks**: the bar's flyouts never extend below the work area's
  bottom; a maximized window sits between the two strips.
- **On the owner's desktop**, with the owner, after the release that carries
  tickets 149–152 and 156–158: turn the bar on from Better Manager, compare it
  with every mockup in `design/top-bar/`, check the tray switch (record which
  items came back, StatusNotifierItems and XEmbed icons separately), a click
  and a menu on each XEmbed icon the owner has, the key bindings, a fullscreen
  video, the overview, and turn it off and confirm the taskbar, GNOME's panel,
  the tray (XEmbed icons back in Zorin's taskbar), and the key bindings are as
  before. Never installed on the host before release (AGENTS.md).

## Done when

- [ ] Unit tests for the `panel-element-positions` edit (several monitors,
      unknown elements kept, a value changed elsewhere not overwritten).
- [ ] In a headless GNOME Shell 46 with Zorin's taskbar: on, the bottom panel
      has no clock or system menu and the bar has them; off, the setting reads
      back byte for byte as before. Screenshots in
      `/home/tim/Developer/better-os-evidence/159/`.
- [ ] The owner's desktop check above, with its results in this ticket's
      Notes and screenshots the owner took.
- [ ] fmt, clippy on stable, and the crates' tests pass.
