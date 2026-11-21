# Ticket 160 — The top bar in the Better OS session

**Spec:** `engineering/specs/better-os-session.md` S4; `design/top-bar-spec.md` section 11
**Blocked by:** 152, 203

Rewritten on 2026-11-21 for ADR 0038's option D, which makes this ticket the
bar's main path ("build (the main path)" in ADR 0038's table). Before, it was
"the top bar on COSMIC", blocked on ticket 143's decision to support COSMIC
as a second desktop; that decision is now the Better OS session on
`cosmic-comp`, where there is no COSMIC panel to hide (decision C7 does not
arise).

## Why

Ticket 152 builds the bar's service and view on layer-shell with every part
that reads or acts on the desktop behind a trait. In the Better OS session
those parts come from `cosmic-comp`'s protocols, logind, Fcitx 5, and Better
OS's own services instead of GNOME Shell.

## What to build

- **First, bring `design/top-bar-spec.md` section 11 up to date** with this
  ticket (written with whoever holds that spec): the session's sources below,
  the session's rows of the shared states table (section 2), and section 14's
  rows for the session.
- **Focused window and its actions** (spec section 4): the activated toplevel
  from `zcosmic_toplevel_info_v1` (app id, title, states), the application's
  name and "can open a new window" from `app-catalog-core` by app id; full
  screen, minimize, and close through `zcosmic_toplevel_manager_v1`; New
  window through the desktop entry's launch path (`--new-window` action where
  the entry declares one); Quit as close of every window of the application;
  the Desktop state when nothing is activated, its Show All Windows opening
  the overview (ticket 212).
- **The Better OS menu** (spec section 3) in the session: Sleep through logind
  as on GNOME; Restart, Shut down, and Log out through a Better OS end-session
  dialog of the bar's own (the 60-second countdown, other people logged in,
  applications inhibiting logout or shutdown through logind) calling logind's
  `Reboot`/`PowerOff` and the session launcher's `org.betteros.Session1.Logout`;
  Lock screen through logind's `LockSession`; the lockdown keys read through
  the session's dconf profile.
- **Input method** (spec section 6.2): Fcitx 5's controller interface on the
  session bus for the groups, input methods, the current one, and the engine's
  status actions; switching through it; the settings link to `fcitx5-config-qt`.
  First confirm the interface's names at the version Ubuntu 24.04 ships and
  record them in Notes.
- **Settings links** in the session (network, sound, power, date and time) open
  COSMIC Settings' pages (C9).
- **Clock settings** (spec section 6.6) in the session: read the time format
  COSMIC Settings writes if it has one (`com.system76.CosmicAppletTime` was seen
  on the bus, ADR 0038's evidence), else GNOME's keys through the session's
  dconf profile, and say which in Notes.
- **Key bindings**: the bar's `--focus` (Ctrl+F2) and `--notifications`
  (Super+V, Super+M; ticket 216) in the session's defaults (ticket 203), not
  through Better Defaults: `Spawn("…")` lines in
  `packaging/session/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults`
  (Super+M is COSMIC's Maximize there today; this ticket decides which
  wins), and the bar's program added to
  `session_launcher::callers::LOGOUT_CALLERS` so its end-session dialog may
  call `org.betteros.Session1.Logout`.
- **No panel to hide, no relay**: the service in the session asks for no
  relay and the card shows no adapter state.

## Done when

- [ ] Unit tests for each source with fakes (toplevel info and manager, logind,
      the session launcher, Fcitx 5's controller), every state and string in
      both languages, the end-session dialog's model.
- [ ] In the VM session from the built packages: the bar's name and actions
      follow focus across a GTK, a Firefox, and a Better OS window; each Better
      OS menu item works (Restart and Shut down cancelled from the dialog);
      the input method switches between Chewing and English; screenshots light
      and dark, zh-TW and English, in
      `/home/tim/Developer/better-os-evidence/160/`.
- [ ] fmt, clippy on stable, and the crates' tests pass.

## Notes

Ticket 229 (`design/system-settings-spec.md`) moves these settings into Better Manager: each settings link here opens the Better Manager page (section 4.3's `--page` names) once its ticket (232, 234, 236, 237) lands, and COSMIC Settings only until then. The clock format the bar reads in the session stays `com.system76.CosmicAppletTime`, which Better Manager's Date and time page writes (ticket 237).
