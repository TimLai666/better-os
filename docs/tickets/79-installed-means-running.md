# Ticket 79 — An installed component runs, with nothing for the person to type

**Blocked by:** none · **Status:** ready

## Why

On the owner's machine Better Awake, Better Monitor, and Better Touchpad were
installed and shown as enabled and healthy, and none of their background
services had ever run. Better Awake's tray icon never appeared, because the
tray asks the service on start, gets no answer, and exits; the touchpad
gestures never worked. The packages install their systemd user units without
enabling them, by design: enabling is Better Manager's step. That step was
never written. `DesiredOperation::Enable` in `manager-core` sets
`record.enabled = true` and nothing else, and install and update do the same.

The owner's rule: installing a component makes it work. Nobody is asked to
run a command.

## What to build

1. **Which units a component has.** The `.service` files its package
   installed under `/usr/lib/systemd/user/`, read from dpkg's file list
   (`PackageStateProbe::installed_files`). No list of unit names is kept by
   component ID, and the manifest schema does not change.
2. **A user-session unit controller** in `manager-platform`, next to the dpkg
   probe: a trait with `enable_now(&[unit])`, `disable_now(&[unit])`, and
   `states(&[unit]) -> (enabled, active)` per unit; a real implementation
   that runs `systemctl --user` with an argument vector, no shell, unit names
   validated as plain `name.service`; and a fake for tests. It needs no
   privilege and never goes through `manager-daemon`.
3. **The driver uses it.** `RealDriver` gains it through a builder method
   added beside `new` (`new`'s signature does not change). After a successful
   install, update, enable, or restore that leaves the component enabled, its
   units are enabled and started before the health stage; disable stops and
   disables them; remove stops and disables them before the package goes. A
   unit that fails to start fails the step with evidence
   `user_units.failed` and the unit and `systemctl`'s message in the detail,
   in both languages in the window, and the component is not marked healthy.
4. **Health includes it.** For an enabled component, the health check also
   requires every unit to be enabled and active. A unit that is not is
   reported by name, not as a generic failure.
5. **Existing machines heal on their own.** When Better Manager starts (GUI
   and CLI), every component it records as enabled whose units are not
   enabled and running has them enabled and started, and the activity log
   says so. That is the recorded intent, so it is not asked again. This is
   what fixes the owner's machine after the update, with nothing typed.
6. Wiring in `manager-gui` and `manager-cli` so the real controller is used
   wherever `RealDriver` is built.

## Out of scope

- The windows' look (the Fluent pass is its own ticket).
- Units of the system bus or system manager; only user units exist today.

## Acceptance

- [ ] With the fake controller and fake dpkg file lists: install, update,
      enable, restore, disable, remove, and a start failure each do what
      section 3 says, and health reports a stopped unit by name.
- [ ] Startup reconciliation enables and starts the units of an enabled
      component and leaves a disabled one alone.
- [ ] The real controller is exercised against a throwaway user manager or,
      where that is not possible in tests, its argument vectors are checked
      exactly; it never runs a shell.
- [ ] The units of `better-awake`, `better-monitor`, `better-storage`, and
      `better-touchpad` are found from the real dpkg file lists of the 0.2.9
      packages (`dpkg-deb -c` on the release assets).
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
