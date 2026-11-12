# Ticket 91 — Control center: retire the separate windows and ship it

**Blocked by:** 85, 86, 87, 88, 89, 90 · **Status:** ready

## Why

Once every page exists, the old windows go, the packages change as ADR 0017
says, and an upgrade from v0.2.9 has to leave the person with working pages,
a working tray, a working safe-mode command, and their settings intact.

## What to build

The "The components stay; their windows go" and "On upgrade" sections of
ADR 0017.

- **Better Awake package:** stop building and shipping `awake-gui`; remove
  `crates/awake-gui` once its model and tests have moved (ticket 87);
  `better-awake.desktop` becomes the shortcut with
  `Exec=better-manager --page keep-awake` and `StartupNotify=false`; remove
  `/usr/bin/awake-gui` from `components/manifests/better-awake.yaml` paths;
  add the dependency on the Better Manager release that carries the page.
- **Better Touchpad package:** `better-touchpad` becomes the small command
  ADR 0017 describes (no GPUI): no argument runs `better-manager --page
  touchpad`; `--safe-mode`, `--normal-mode`, `--help`, `--version` as today;
  everything else, including the dropped `--lang`, `--page`, and
  `--offline`, refused in both languages. `better-touchpad.desktop` becomes
  the shortcut; the safe-mode entry is unchanged. Manifest dependency added.
- **Better Manager package:** `better-manager --page` documented in its
  `--help`; `io.betteros.Manager.desktop` keywords gain the page names in
  both languages so the launcher finds them.
- **Packaging checks:** `packaging/build-deb.sh` and `verify-deb.sh` updated
  for the removed binary and the new command; `verify-deb.sh` asserts each
  shortcut entry's `Exec` names `better-manager --page`.
- **Docs:** `AGENTS.md` (no more `awake-gui` or touchpad window; the control
  center), `docs/architecture.md`, `delivery-status.md`, the touchpad and
  awake release notes in their manifests.
- **Release gate:** a test fails if any page is still the debug-only
  placeholder from ticket 84.
- **Upgrade test in a container (Chefer AppCipe), never on the host:**
  install the v0.2.9 packages, add a rule, a quick-session length, and a
  touchpad setting, upgrade to this build, and show: the tray's Open runs the
  control center on Keep awake; the rule and the length are there; the
  touchpad setting and its capture are unchanged; `better-touchpad
  --safe-mode` still writes the marker; rolling Better Awake back to 0.2.9
  brings `awake-gui` back and it still talks to the service.

## Acceptance

- [ ] Every ADR 0017 package and upgrade statement is shown by the container
      test or a packaging check.
- [ ] Spec section 12 rechecked row by row against the shipped build, with
      the result recorded here.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      `verify-deb.sh`, and the license check pass.

## Notes

Not in scope, found while designing: the Better Awake manifest's release notes
promise a choice to delete rules and history at uninstall, and nothing offers
it. It needs its own ticket.

Whether `manager-core`'s dependency resolution updates Better Manager before
a component that depends on a newer one is to be checked here; if it does
not, the dependency is also declared in each `.deb`'s `Depends:` so apt
refuses the wrong order.
