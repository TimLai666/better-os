# Ticket 208 — The session starts with the person's wallpaper, appearance, scale, layouts, and input sources

**Spec:** better-os-session.md S1 ("Carrying settings over")
**Blocked by:** 203, 206, and the owner's decision S1

## Why

The owner's bar: one action and nothing else to set up. A session that opens
with another wallpaper, light where the person chose dark, the wrong scale,
or no Zhuyin is a second step the person has to know about.

## What to build

- **Each row of the spec's carry-over table**, read from GNOME (its dconf
  database read directly, `~/.config/monitors.xml`, IBus's settings) and
  written to the session's own place through ticket 206's adapter or, for
  Fcitx 5's profile and keys, a typed writer of its own in `defaults-platform`
  with the same capture and restore; never a write to GNOME's configuration.
- **When**: at install and at every Use Better OS (decision S1), captured
  first so removal restores what was there before.
- **Input sources**: `ibus:chewing` becomes Fcitx 5's Chewing; each IBus
  engine with no Fcitx 5 counterpart is listed on the component page
  (「{engine} 在 Better OS 工作階段沒有對應的輸入法。」 / "{engine} has no
  counterpart in the Better OS session.") rather than dropped silently; the
  switch key is ticket 94's `<Alt>Shift_L`. First find where Fcitx 5 reads its
  trigger and enumerate keys and its profile, and record it in Notes.
- **Scale**: `monitors.xml`'s logical monitors to `cosmic-comp`'s output
  configuration in the format ticket 201 found, matched by connector and
  vendor, model, serial; a monitor not in `monitors.xml` left to the
  compositor.

## Done when

- [ ] Unit tests for each mapping from recorded GNOME values (several
      monitors with fractional scale, both color schemes, `picture-uri` with a
      `file://` URI and a missing file, layouts with variants, IBus engines
      with and without a counterpart).
- [ ] In the VM, a GNOME account set up with a wallpaper, dark mode, 150 %,
      two layouts, and Chewing logs in to the session and sees all of them;
      GNOME's database and `monitors.xml` unchanged byte for byte afterwards.
- [ ] fmt, clippy on stable, and the crates' tests pass.

## Notes

Found by ticket 229 (2026-11-21, read only): the owner's GNOME has no `~/.config/monitors.xml` (Mutter runs its own choice, 2880 × 1800 at 120 Hz, scale 2.0, from `org.gnome.Mutter.DisplayConfig.GetCurrentState`). The scale carry-over must read Mutter's current state when the file is absent, or it carries nothing on that machine.
