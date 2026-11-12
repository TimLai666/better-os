# Ticket 83 — Better Manager as the control center: design

**Blocked by:** 80 · **Status:** done, approved by the owner on 2026-11-12

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

## What was built

- **The spec**, `design/control-center-spec.md`: the window (navigation,
  "Find a setting", back button, `better-manager --page`, one instance), the
  shared states with their copy (loading, not installed, too old, service not
  running, hardware absent or unreadable, no permission, not allowed, write
  failed, read back different), and every page: Home, System, Touchpad, Keep
  awake, Apps, Default apps, Fixes, Settings, with zh-TW and English copy.
  Copy that already existed and worked is kept word for word from each
  window's `i18n.rs`; changes are listed with the audit finding behind them.
- **The mapping** (spec section 12): every function of Better Manager, its
  Defaults pages, Better Awake's window and the tray's window entry points,
  and Better Touchpad's window, with its new place. Three things are
  deliberately dropped, each with the evidence: the auto-download and
  diagnostic-log switches and the Preview update channel, which nothing in
  the code reads, and Better Touchpad's `--lang`, `--page`, and `--offline`.
- **Hardware** (spec section 11): read on the owner's Vivobook without writing
  anything. Two corrections to what the ticket assumed: the keyboard backlight
  needs no root, because gsd-power (`Power.Keyboard`) and UPower
  (`KbdBacklight`) already write it for the session; and power mode needs no
  daemon, because power-profiles-daemon's polkit action allows the active
  session. Only the charge limit and the fan mode go through `manager-daemon`,
  as two typed methods with two polkit actions on a new
  `org.betteros.Hardware1` interface. The daemon's unit has
  `ProtectKernelTunables=yes`, which makes `/sys` read-only; the spec keeps it
  and opens the two attributes with `ReadWritePaths=`, to be proven in a VM
  first.
- **ADR 0017**, `docs/decisions/0017-one-control-center.md`: one window; the
  Awake and Touchpad packages keep their services, tray, extension, and
  safe-mode command and lose their windows; their desktop entries become
  shortcuts to the pages; the awake window's preferences move into the awake
  service; manifests depend on the manager release that carries the pages;
  the daemon gains the hardware interface (amending ADR 0007); Better Patch is
  a page with a GPUI-free engine and typed entries.
- **Tickets 84–91.** 84 builds the shell, Home, Settings, and the kit
  additions; 85 (Apps), 86 (System and hardware), 87 (Keep awake), 88
  (Touchpad), 89 (Default apps), and 90 (Fixes) can then run in parallel,
  each in its own page module; 91 retires the old windows and proves the
  upgrade in a container. 86's crate and daemon work can start before 84.
- **Mockups**, `design/control-center/`: Home, System, and Keep awake in light
  and dark, plain HTML and CSS with no external resource, using DESIGN.md's
  colour values and the machine's real readings.

Found while reading, and put into the tickets: the Quick sessions list the
awake window edits is never read by the tray, which hard-codes its lengths
(87); the tray's "Until a time…" opens a window that cannot start such a
session (87); the "check for updates when opened" switch is ignored at launch
(85); the awake manifest promises a delete-my-data choice at uninstall that
nothing offers (noted in 91, out of scope).

Not done or not verified:

- Nothing was written to the machine. Whether the charge limit survives a
  reboot, the exact `pwm1_enable` values on `asus-nb-wmi` (0 full, 2 automatic
  is recalled from the driver, not read from its source), and whether UPower's
  `KbdBacklight.SetBrightness` is polkit-gated are open and owned by ticket 86.
- Whether `ReadWritePaths=` overrides `ProtectKernelTunables=` under systemd
  255 is not shown; ticket 86 must show it in a VM.
- The Touchpad, Apps, Default apps, Fixes, and Settings pages have no mockup;
  their layout is in the spec only. The mockups show the expanded pane and the
  48px pane but not the ☰ toggle in the compact pane.
- gpui-component's icon set has no house or touchpad glyph; the spec uses
  `LayoutDashboard` and `Frame` and leaves the choice to the owner.
- The zh-TW Windows 11 wording for the power modes (最佳電源效率 / 平衡 /
  最佳效能) is from memory of Windows 11, not checked on a Windows machine.

### Screenshots

In `/tmp/claude-1000/-home-tim-Developer-better-os/3b559518-c3b2-4104-bc77-d1c2e845c29a/scratchpad/shots-83/`:

- `gallery-light-settings-cards.png` — `better-fluent-gallery --light --page
  settings` in the headless sway sandbox, checked against the mockups.
- `mock-home-light.png`, `mock-home-dark.png`, `mock-home-dark-960.png`,
  `mock-system-light.png`, `mock-system-dark.png`, `mock-awake-light.png`,
  `mock-awake-dark.png` — the mockups rendered by headless Chrome at
  1400 × 1000 (and 960 × 800 for the compact pane).
