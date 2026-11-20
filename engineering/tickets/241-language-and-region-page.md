# Ticket 241 — Language and region in Better Manager

**Spec:** `design/system-settings-spec.md` section 13 (Language and region); ADR 0042 (proposed)
**Blocked by:** 231, 203, the owner's decision M6, and the owner's decision M1

## Why

COSMIC Settings sets the **system** locale through localed when the person
picks a language (`pages/time/region.rs` 843–864); GNOME keeps it per person in
AccountsService. Better OS keeps it per person in both.

## What to build

- First check, in the VM, what GDM exports for a non-GNOME session from
  AccountsService `Language` and `FormatsLocale`; record it in Notes and, if
  formats are not exported, make the session launcher (ticket 203) export
  `LC_*` from `FormatsLocale`.
- Spec section first, then Time and language › Language and region: display
  language order (`SetLanguages`), formats (`SetFormatsLocale`, and
  `org.gnome.system.locale region` under GNOME), the next-sign-in note,
  installing a language by launching `gnome-language-selector` until a daemon
  method is decided.
- Better Manager's own Language card keeps working and says what it is
  (section 13's last paragraph).

## Done when

- [ ] Tests against a fake AccountsService; the sign-in note; an
      uninstalled language.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/241/`. A sign-in after the change shows the new language and formats in a GTK program and a Better OS window.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
