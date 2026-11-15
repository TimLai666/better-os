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

## Notes

- Design: [ADR 0028](../../decisions/0028-daemon-upgrades-outside-its-own-unit.md).
  The service is a companion of `better-manager` in its manifest; the client
  picks the path from dpkg's version of the service
  (`manager_core::exec::ServiceGeneration`).
- Built: requirements 1, 2, 3, 4 and 5 in `manager-core` (translation,
  `AddCompanions`, `companion_gaps`/`companion_checks`, Verify checks
  companions), `manager-daemon` (own package last, `systemd-run --wait` in
  `better-manager-daemon-upgrade`), `packaging/maintainer/daemon-handoff.sh`
  and its postinst hook, the Updates notice, the window-attention Install
  button, Doctor findings in the GUI and the CLI.
- A failed handoff with Better Manager current is retried from the window:
  the missing adapter is installed through the old service as a step of its
  own, and `better-shell-adapter`'s postinst starts the same handoff
  (ADR 0028). Only when the adapter is already installed and the service is
  still 0.2.12 or older is there no retry; the notice says so.
- On ticket 85's update flow (merged from main at `aff7c13`): there is no
  review page. 「更新系統服務」 and the window-attention Install start the
  change at once; the card shows Better Manager's progress in place of its
  button, and after a failure the reason with a Retry that starts the same
  change again (`service_change_card`). The row of an Add companions change
  names the parts it adds, says "Missing parts added" when done, and never
  shows the "This updates Better Manager itself" notice. The companion
  finding is on the Health card with display names.
- Fixed while porting, in ticket 85's code: a change that never reached the
  service (or was cancelled) left its plan as the rows' result, so the row
  read "Missing parts added" / "Updated" over a change that did nothing.
  A row now shows a result only when the change's last record is a success
  (`ended_in_success`), and a missing service says so in a banner
  (`AppError::ServiceUnavailable`).
- Looked at again in an own headless GNOME Shell 46 (private session bus,
  PipeWire, fake `dpkg-query` and `systemctl`, no system bus, systemctl
  guard), in Traditional Chinese and English, light and dark: the Updates
  notice, the same page after pressing it, the window-attention Install and
  the page after pressing it, the Health card, and the after-install states.
- Buttons were pressed from the keyboard. Pointer clicks still reach no
  application window in a headless GNOME Shell, a GTK one (GNOME
  Calculator) included, with relative and with absolute (screencast-stream)
  pointer motion, while the shell's own panel takes them; so the session's
  synthetic pointer is the cause, not the window. Ticket 85's click fix was
  proved under sway; a mouse click on these buttons has not been seen.
- The installation itself ran against no daemon: the after-install state was
  produced by putting the adapter's files and dpkg record in place.
- The container end-to-end section (0.2.9 service, handoff, adapter,
  `dpkg --audit` empty) is CI-verified after merge: there is no Docker
  daemon here.
- `SELF_UPGRADING_SINCE` and `HANDOFF_SINCE` say 0.2.16; the version bump
  must keep them true.
- Checks run on `b6b541a` (main `aff7c13` merged): fmt, clippy (newest
  stable and 1.99), check on 1.95, the maintainer-script and daemon-handoff
  tests, the license inventory check, and `build-deb.sh` plus
  `verify-deb.sh` for ubuntu-24.04 amd64 all passed. In the gate's test run
  `files-operations`' `extracting_pauses_resumes_and_cancels_leaving_no_folder`
  failed once under load (a pause that came after the extraction finished);
  it passed 10 of 10 alone, and the full `cargo test --workspace --locked`
  rerun passed: 279 suites, 4705 passed, 0 failed.
