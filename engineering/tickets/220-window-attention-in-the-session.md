# Ticket 220 — Window attention's default choice works in the session, and per-application choices say why they don't

**Spec:** better-os-session.md S5 (window attention row)
**Blocked by:** 206

## Why

`cosmic-comp` has one machine-wide `activation_policy` (`Focus`,
`FocusIfActiveWorkspace`, `Urgent`) in `com.system76.CosmicComp`, no urgent
state in `zcosmic_toplevel_info_v1`, and no per-application hook (ADR 0038).
`better-window-attention.service` also carries the charger switch, which must
keep working in both sessions.

## What to build

- **The default choice** mapped to `activation_policy` through ticket 206's
  adapter in the session, captured and restored like any default.
- **Per-application choices**: shown unavailable in the session with
  這個桌面不支援為個別應用程式設定。 / "This desktop doesn't support a setting
  per app.", kept in the store untouched so GNOME still applies them.
- **The service** runs its GNOME relay counterpart only under GNOME and the
  charger switch in both, decided at run time by `better_core::desktop`.

## Done when

- [ ] Unit tests for the mapping of each default and the page model in both
      sessions and both languages.
- [ ] In the VM session, the default changes `activation_policy`; the
      charger switch follows a fake UPower plug event.
- [ ] fmt, clippy on stable, and the crates' tests pass.
