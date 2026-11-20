# Ticket 159 — The top bar with the bottom taskbar, and on the owner's desktop

**Blocked by:** 152, 156, and the owner's decisions D3 and D4 in
`design/top-bar-spec.md`

## Why

`design/top-bar-spec.md` section 10: while the bar is on, the bottom taskbar
must not repeat what the bar shows, the two must never overlap, and turning the
bar off must leave the taskbar exactly as it was. The design also has to be
seen once on the owner's own desktop, at scale 2, with the person's own
taskbar.

## What to build

- **Decision D3 (a)**, the recommendation: a Better Defaults entry "Taskbar
  without clock and system menu" that sets `visible: false` on `dateMenu` and
  `systemMenu` in every monitor's entry of
  `org.gnome.shell.extensions.zorin-taskbar panel-element-positions`, keeping
  every other element and the JSON's order, captured and restored exactly;
  "nothing to do" without Zorin's taskbar. On Ubuntu, Ubuntu Dock has neither,
  so nothing is written.
- **If the owner chooses (b)**: the entry also sets Zorin's taskbar to a
  centred, intellihiding dock (the keys named in its schema, each captured and
  restored), instead of the above.
- **If the owner chooses (c)**: nothing here; ticket 142's taskbar adds its
  row to ADR 0036's program table, uses `ReserveStrip(monitor, 1, px)`, and
  draws no clock, system menu, or tray while the bar runs. Record D4's answer in
  ticket 142 so it is built that way.
- **Geometry checks**: the bar's flyouts never extend below the work area's
  bottom; a maximized window sits between the two strips.
- **On the owner's desktop**, with the owner, after the release that carries
  tickets 151–158: turn the bar on from Better Manager, compare it with every
  mockup in `design/top-bar/`, check the tray switch (record which items came
  back), the key bindings, a fullscreen video, the overview, and turn it off
  and confirm the taskbar, GNOME's panel, the tray, and the key bindings are as
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
