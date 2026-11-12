# Ticket 83 — Better Manager as the control center: design

**Blocked by:** 80 · **Status:** ready

## Why

The owner decided (ticket 76, direction 6) that Better Manager becomes the one
control center, laid out like Windows 11 Settings, absorbing Better Awake,
Better Touchpad, Better Defaults, Better Patch, and hardware controls. This
ticket designs it completely before anyone builds it.

## What to deliver

1. `design/control-center-spec.md`: every page, every setting card, every
   state (loading, empty, error, no permission, hardware not present), and
   the exact zh-TW and English copy. Pages at least: Home (what needs
   attention), Apps (install, update, remove components; today's manager
   pages), System (power mode, battery charge limit, display brightness,
   keyboard backlight, fans and temperatures), Keep awake (all of today's
   Better Awake: quick sessions, rules, battery safety, history), Touchpad
   (all of today's Better Touchpad), Default apps (all of Better Defaults),
   Fixes (Better Patch's known faults), and Settings (appearance, language,
   update channel, about).
2. A mapping of every function in the four existing windows to its new place,
   so nothing is lost.
3. Hardware: which controls appear only when the hardware has them, and how
   each is written. Facts already gathered on the owner's ASUS Vivobook S 14
   M5406WA: power mode must go through power-profiles-daemon
   (`net.hadess.PowerProfiles`), not `platform_profile`; charge limit
   `/sys/class/power_supply/BAT1/charge_control_end_threshold` (root);
   keyboard backlight `asus::kbd_backlight`, single colour 0–3 (root);
   brightness should go through the desktop's own path; fan is read-only
   except auto/full (`pwm1_enable`); no RGB, no fan curve. Root writes go
   through `manager-daemon` and polkit with a typed action per control.
4. The ADR that replaces "separate apps" for Awake, Touchpad, and Defaults,
   including what happens to their packages, tray, and services on upgrade.
5. The implementation tickets that follow, each complete in itself, ordered so
   several can run in parallel.

Use `better_ui::fluent` controls only, and the gallery (`better-fluent-gallery`)
to check what exists.
