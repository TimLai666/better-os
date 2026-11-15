# ADR 0028: The privileged service is upgraded outside its own unit

## Status

Accepted on 2026-11-15 (ticket 124).

## Context

Nothing updated `better-manager-daemon`. It has no manifest, so a machine
installed with 0.2.9 kept a 0.2.9 service while every component moved to
0.2.15. That service refuses a plan carrying `companions` (ADR 0018), and the
client resent the plan without them, so `better-shell-adapter` was dropped and
the update was reported as a success.

Updating the service through itself is the dangerous case AGENTS.md records:
dpkg runs the *installed* version's prerm, and up to 0.2.12 that prerm runs
`systemctl stop better-manager-daemon.service`. Inside the daemon's own unit,
that stop kills the dpkg run executing the script and leaves the package
half-installed. 0.2.13 and later skip the stop on an upgrade and skip the
postinst restart inside the unit (ticket 105), so for them an in-unit upgrade
is safe but still runs dpkg under the daemon's systemd sandbox.

## Decision

1. **The service travels as a companion of Better Manager.** Every artifact in
   `better-manager.yaml` lists `better-manager-daemon` at the same version,
   beside `better-shell-adapter`. `better-manager` already recommends both at
   `= VERSION`. No new manifest and no schema change.
2. **The client chooses the path from dpkg's version of the service**, read
   unprivileged with `dpkg-query` (`manager_core::exec::ServiceGeneration`):
   - **0.2.16 and later** (`SelfUpgrading`): the service is sent as its own
     step, last in the plan. The service runs that one `apt-get` with
     `systemd-run --wait --collect --pipe` in a transient system unit, so
     dpkg runs outside the service's cgroup and sandbox. Its postinst sees the
     busy marker and does not restart it; the service leaves ten seconds after
     the transaction and D-Bus starts the new one (ticket 105).
   - **0.2.14 and 0.2.15** (`Companions`): the same separate step, run inside
     the unit by that service's own code. Their prerm and postinst are the
     ticket 105 scripts, which neither stop nor restart the unit there.
   - **0.2.13** (`Stepwise`): the same separate step; it does not know
     `companions`, so the adapter is deferred, said so in the transaction's
     evidence, and offered with **Install** once the new service runs.
   - **0.2.12 and older** (`Legacy`): the service is never sent as a step.
     The client stages its package and every companion in the old service's
     archive (staging needs no plan), and sends the Better Manager step alone.
     The new `better-manager` postinst finds a staged service package whose
     checksum is the one built into it, and starts
     `/usr/libexec/better-os/daemon-handoff` with `systemd-run --no-block` in
     `better-manager-daemon-handoff.service`. That unit waits for the old
     service to go idle, then installs the service and the staged adapter in
     one `apt-get` run from outside the old unit, where the old prerm's stop
     is harmless. It writes its state to `/run/better-os/daemon-handoff`
     (world-readable), which the window reads. This needs a Better Manager
     update in the same plan; the handoff ships from 0.2.16.
3. **No silent workaround.** The retry that resent a plan without its
   companions is gone. A plan whose companions the service cannot take, and
   which does not itself update the service, is refused before anything is
   sent with `daemon.outdated`, and the window shows 「需要先更新系統服務」
   with **Update system service**. A refusal that names `companions` at run
   time maps to the same key.
4. **A missing companion is a finding.** Doctor reports an installed
   component's companion that dpkg does not have, or has at an older version,
   as `companion_missing`; `Manager::plan_companions` plans the repair, an
   update of the component to its own version carrying only the missing
   companions, through the same daemon path.

## Options rejected

- **A manifest for the service.** It would be a catalog row for a package
  nobody chooses, with enable, disable, and remove that mean nothing for it,
  and the remote catalog's fixed file list means no released manager would
  fetch it.
- **A pkexec helper run by the window.** It is privileged mutation outside
  `manager-daemon` with its own polkit action and argument parsing, exactly
  the surface ADR 0007 keeps in one place.
- **Sending the service as a step to a 0.2.12 service.** That is the
  half-installed package AGENTS.md records.

## Consequences

- Without systemd (the container check), there is no unit to be killed by:
  the service runs apt directly and the handoff runs detached with `setsid`.
  Both say which they did in their log.
- A machine whose 0.2.12-or-older service missed the handoff while Better
  Manager is already current has no in-window repair: the old service cannot
  reinstall the same Better Manager version. The window says so and names
  the handoff's recorded failure; the next Better Manager release repairs it.
- `SELF_UPGRADING_SINCE` in `manager_core::exec` is `0.2.16`. A release that
  is not 0.2.16 must change it in the version-bump commit.
- A released 0.2.15 manager refreshing the catalog from `main` sends the
  service as a companion. A 0.2.14 or 0.2.15 service installs it in its unit,
  safely (ticket 105); an older one refuses and that manager drops the
  companions as before. Only 0.2.16 managers carry this decision.
