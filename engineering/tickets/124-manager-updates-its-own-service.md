# 124 — Better Manager keeps its own service and the shell adapter up to date

**Epic:** Field reports from the first real Zorin 18 install
**User Story:** A person who updates everything in Better Manager ends up with
every Better OS package at the new version, including the privileged service
and the GNOME Shell adapter, and is told plainly when something could not be
done and offered the button that does it.
**Blocked by:** none

## What was seen (owner's machine, 2026-11-15)

- Every component is 0.2.15; `better-manager-daemon` is still 0.2.9 and
  `better-shell-adapter` is not installed.
- The window-attention page says the GNOME extension is missing and tells the
  person to rerun the installer.
- Cause: the 0.2.9 daemon rejects a plan carrying `companions`
  (`deny_unknown_fields`), and `crates/manager-core/src/exec.rs` (the
  `refuses_companions` retry) resends the plan without them. The update reports
  success and the adapter is silently dropped.
- Nothing updates the daemon: `components/manifests/` has no manifest for it.

## What has to be true

1. **The service is updated with the manager.** Better Manager plans an update
   of `better-manager-daemon` whenever a newer published version exists. The
   upgrade must never stop or restart the daemon from inside its own unit
   (AGENTS.md: a stop inside the unit kills the dpkg run). A daemon new enough
   runs the upgrade in a unit outside its own cgroup, so its own prerm cannot
   kill the transaction; record the design in an ADR before building it.
2. **An old service is never worked around silently.** When the daemon refuses
   `companions` (or any field the plan needs), the affected rows show
   「需要先更新系統服務」 with an **Update system service** button, and the
   companions are not dropped without saying so. The silent retry goes.
3. **A daemon too old to update itself safely (0.2.12 or older)** is updated by
   a path that does not run inside its own unit; the row says what will happen
   and asks for the one admin prompt. No terminal command is shown as the only
   way out.
4. **A missing companion is installed from where it is missing.** The
   window-attention page (and any other page that depends on
   `better-shell-adapter`) shows **Install** instead of "rerun the installer"
   when the adapter is missing, goes through the same daemon path, and says to
   log out and back in only when GNOME actually needs it to load the extension.
5. Verify, Doctor, and the health check report a missing companion of an
   installed component as a finding, not as healthy.

## Done when

- Tests cover: an old daemon refusing companions shows the row state and no
  silent retry; the daemon plan is offered; the missing-adapter Install path;
  Doctor's finding.
- The container end-to-end check upgrades from a 0.2.9 daemon to the new one
  through Better Manager and finishes with the adapter installed and dpkg
  consistent (`dpkg --audit` empty).
- The screens were looked at in a sandbox.
