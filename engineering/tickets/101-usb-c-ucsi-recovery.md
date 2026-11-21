# Ticket 101 — The USB-C ports keep working on Linux

**Blocked by:** none

**Blocked:** the measurement before and after needs the next release
installed on the owner's machine and a real occurrence of either fault
(see Notes).

**Owner's decision (2026-11-21):** no Machine Owner Key is enrolled and Secure
Boot stays on (it is on: `mokutil --sb-state` reports enabled). So nothing in
this ticket may need a module the kernel would refuse: part 2's DKMS module
is withdrawn, and the recovery of part 1 has to do the whole job with what
the running kernel already has. What is left of part 2 is the upstream
report only (see its heading). Reloading the distribution's own
signed `ucsi_acpi` stays allowed under Secure Boot (ADR 0046).

## Why

On the owner's ASUS Vivobook S 14 M5406WA (BIOS M5406WA.314, kernel
7.0.0-42, Secure Boot on) the faster USB-C port sometimes stops charging under
Linux and never under Windows. The kernel log shows the UCSI interface timing
out: `ucsi_handle_connector_change: GET_CONNECTOR_STATUS failed (-110)` and,
after resume, `failed to re-enable notifications (-110)`.

The same fault on the sibling model, ASUS Vivobook S 15 M5506WA (same AMD
`CPMUCSI` SSDT, BIOS M5506WA.316, kernel 7.0.12), was reported to linux-usb on
2026-06-23 with no reply as of 2026-11-13: the second port stops charging,
the errors repeat after every suspend, and reloading `ucsi_acpi` restores both
ports until the next suspend. No ASUS BIOS update addresses it, and no
upstream patch for these models was found
(https://lore.kernel.org/linux-usb/CAAF7Zqye8YCxpCXcVPbAQzsAiF7upRySyFP959Wio+07rRS5nA@mail.gmail.com/).
A May 2026 patch that disables `ucsi_acpi` on some Lenovo models with broken
firmware is not a fix here: it would lose the ports' Linux-side state.

## Part 1 — recovery, shipped as a Better Patch entry

- Detect: the `ucsi_acpi` -110 errors in the kernel log since boot, on a
  machine whose DMI matches a known affected model (M5406WA, M5506WA to start).
- Recover automatically after resume: a `systemd-sleep` post hook installed
  by the privileged package reloads `ucsi_acpi` when the errors are present,
  and the control center offers "Reset USB-C" for any time in between. The
  reload runs in `manager-daemon` through a typed action with its own polkit
  rule, never a shell.
- Verify: after the reload, both `/sys/class/typec/port*` are present and the
  connected charger shows `online=1` again.
- Undo: removing the entry removes the hook.
- Measured on the owner's machine before and after.

## Part 2 — withdrawn as a local fix; the upstream report stays

The owner decided on 2026-11-21 that no key is enrolled, so a patched
`ucsi_acpi` cannot be loaded on this machine and the steps below that build
and load one are not done. What remains: collect the traces part 1's
detection already reads, and send them to linux-usb as a report on the
existing M5506WA thread, without a patch that nobody could test here. The
text below is kept as the record of what was planned.


- Reproduce on the owner's machine with tracing (`ucsi` tracepoints, dynamic
  debug for `drivers/usb/typec/ucsi`) across suspend and plug events, and find
  which command times out first and why (resume ordering, PPM reset, the
  EC's response time).
- Write the fix as an upstream-shaped patch to `ucsi_acpi` (for example a DMI
  quirk that resets the PPM on resume or waits for the EC), built as a DKMS
  module for testing. Secure Boot is on, so loading it needs a Machine Owner
  Key the owner enrolls once at boot; that step is the owner's and is not
  taken without asking.
- Send the patch to linux-usb, referencing the M5506WA report, so a later
  kernel fixes every affected machine; Better OS ships the DKMS module only
  until a released kernel carries the fix.

## The xHCI controller dies on resume and blocks every later suspend (2026-11-14)

The owner's network dropped again and again overnight. The journal of the
boot that started 2026-11-13 11:38 shows the cause:

- At 19:25:28, resuming from suspend, the USB controller at `0000:63:00.4`
  (AMD 151e, `xhci_hcd`) failed: `AMD-Vi: Event logged [IO_PAGE_FAULT
  domain=0x0014 address=0x0]`, `WARN: xHC restore state timeout`, `PCI
  post-resume error -110!`, `HC died; cleaning up`, then `usb 1-1: USB
  disconnect`.
- From then on a `usb_hub_wq` work item (`hub_event`) never finished. At
  00:09:25 the lid was closed; every suspend attempt after that failed with
  `Freezing remaining freezable tasks failed after 20 seconds ... wq_busy=1
  ... usb_hub_wq ... in-flight: hub_event for 17056s` and systemd-sleep's
  `Device or resource busy`. logind retried about every 40 seconds until the
  lid was opened at 11:01:56: 1,672 suspend entries in 11 hours, each one
  deauthenticating the Wi-Fi (`wlp98s0: deauthenticating ... DEAUTH_LEAVING`),
  which is the overnight network loss. The laptop stayed awake, lid closed,
  all night.
- Reading `/sys/bus/usb/devices/usb1/product` afterwards blocked, so the
  hung work still held the hub. Only a reboot clears it.

This is more than the UCSI `-110` already recorded: the whole xHCI host for
one USB-C side dies on resume. Part 1 gains two things:

- **Detect it.** The recovery checks the kernel log of the current boot for
  `HC died` / `post-resume error` on an xHCI device and for a hub_event that
  never ends (a failed freeze naming `usb_hub_wq`), and reports it in Better
  Manager's Fixes with plain words: USB on that side stopped working after
  sleep, sleep is now blocked, and a restart fixes it.
- **Recover without a reboot where the kernel allows it.** A typed
  manager-daemon action unbinds and rebinds the dead controller
  (`/sys/bus/pci/drivers/xhci_hcd/unbind` then `bind`, or a PCI remove and
  rescan of that one function), under its own polkit action, only for an
  xHCI device the detection named. If the hung work cannot be released, say
  a restart is needed rather than trying anything else.
- **Stop the overnight loop.** When a suspend fails because of a frozen
  workqueue, do not let the lid keep retrying all night with the Wi-Fi
  bouncing: after a second failed attempt with the same cause, Better OS
  tells the person (notification) and, if the lid is closed, asks logind to
  power off rather than burn the battery in a bag, with the person's
  preference for that in Fixes (default: notify only; power-off is a choice).
- **Part 2's patch** also looks at the resume path of this controller
  (`xhci_resume` restore timeout with an IOMMU page fault at address 0),
  and the upstream report adds this log.

## Notes

### Built (2026-11-22)

Spec: `engineering/specs/better-patch.md`, "Entries 3 and 4". Decision:
ADR 0046 (proposed). Every state and sentence is in zh-TW and English.

- **Detection** (`hardware_core::usb_recovery`, no I/O): the model list as
  data (`crates/hardware-core/data/usb-recovery-machines.txt`: M5406WA,
  M5506WA, controller 1022:151e); the kernel log classifier (UCSI -110,
  `HC died` with the page fault, restore timeout, and post-resume error, hung
  `usb_hub_wq` work, each suspend attempt and the run of attempts that failed
  for that reason, the daemon's recovery markers); what is failing now;
  which recovery applies or that only a restart does; what a recovery
  restored; the overnight guard. `hardware_platform::usb` reads sysfs and
  `/proc` for it under a root (never a USB device's own attribute).
- **Who reads the log:** the Fixes page runs `journalctl` as the person when
  their account can open the system journal (`adm`, `systemd-journal`);
  anyone else gets `org.betteros.UsbRecovery1.Findings`, classified findings
  and never a log line.
- **Recovery:** `ReloadUcsi` (`delete_module` and `finit_module` on the
  distribution's signed file, in a helper unit with `CAP_SYS_MODULE` only)
  and `RebindController` (unbind and bind of the one function the log names,
  in a helper unit that sees the driver's two files), each behind its own
  polkit action, both refusing unless the fault is there, the rebind
  refusing while the hub work is stuck ("restart needed"), 30 s and 60 s
  cooldowns, coalesced. After each: the ports, the charger, or the
  controller's root hubs are looked at again and the answer says what came
  back. `SetChoice` keeps the person's choices.
- **After sleep:** `/usr/lib/systemd/system-sleep/better-os-usb-recovery`
  (one `exec` of `better-manager-daemon --after-sleep "$1" "$2"`) queues
  `better-os-usb-after-sleep` once a fix is on; it reloads after the
  post-resume timeout, rebinds a dead controller, and runs the overnight
  guard: notify after the second failed sleep in a row (default), or power
  off through logind when the person chose it, the lid is closed, nothing
  blocks shutdown, and nobody else is signed in.
- **Fixes:** entry 3 (the USB-C interface) and entry 4 (the dead controller
  and the blocked sleep), Fix, check, Undo (turns the choice off), Reset
  USB-C, the power-off choice on entry 4's card, and "Doesn't apply to this
  PC" with nothing offered on any other model. The fix is kept by the
  machine (the daemon's choice file), not the person's `fixes.json`.
- **Packaging:** the hook and three polkit actions in the daemon package,
  `org.betteros.UsbRecovery1` in the bus policy, `verify-deb.sh` checks
  them, removal forgets the choices, and `packaging/hardware-unit/check.sh`
  covers the two helpers' and the after-sleep unit's sandboxes as far as a
  container can (it never makes a module system call or unbinds a real
  device: the container shares the CI host's kernel).
- **Part 2:** the upstream report, for the owner to send, is
  `engineering/upstream/101-ucsi-and-xhci-on-asus-m5406wa.txt` (both faults,
  the classified traces, the hung-task call trace, a reply to the M5506WA
  thread). No patch, no module.

### Review fixes (2026-11-22)

An independent review of the pull request found four P2 and five P3
problems and one to investigate (its text:
`better-os-evidence/101/review-2026-11-22.md`). All are fixed, each with a
test that fails without the fix:

- The reload helper checks the module file (present, not a link, root's,
  and for a compressed file a kernel config that can decompress it) before
  it unloads anything (`the_module_stays_loaded_unless_its_file_is_ready`).
- The kernel log is read as JSON with each entry's transport: kernel lines
  only from the kernel, and from the daemon's stream only the two exact
  markers; the notification child's output goes to `/dev/null`
  (`only_kernel_lines_and_exact_markers_are_read_from_the_journal`,
  `a_forged_line_in_the_daemons_stream_moves_nothing`).
- Power-off also needs logind's `Docked` to say no, the daemon not in a
  transaction (its busy marker), and no other session of any uid; the card
  says so (`the_overnight_guard_notifies_once_and_powers_off_only_when_chosen_and_safe`,
  `the_overnight_loop_is_told_once_and_powered_off_only_when_chosen`,
  `every_other_session_blocks_the_power_off`).
- A reload is marked only when the charger shows too
  (`a_reload_that_left_the_charger_hidden_is_not_marked`).
- A failed or refused reset holds the cooldown too
  (`a_failed_or_refused_reset_holds_the_cooldown_too`).
- A notification has a five-second call timeout and a ten-second child
  deadline (`a_notification_that_hangs_is_ended_at_its_deadline`).
- `Workqueue: usb_hub_wq` counts as stuck hub work only inside a hung-task
  report (`a_hub_worker_in_a_warning_is_not_hung_hub_work`).
- The notification's language follows the one rule in `better_core::locale`,
  which Better Awake now calls too (`zh_HK` is Traditional Chinese).

### Second review fixes (2026-11-22)

The second review (`better-os-evidence/101/review-2-2026-11-22.md`) found
four P3 and two to investigate in the first round of fixes; all six are
fixed:

- A hung-task report counts only for the worker its own lines name, and
  the query keeps every dump's first lines so a grepped log says the same
  (`a_hub_worker_in_a_warning_is_not_hung_hub_work`, every case grepped).
- Only a run that reached the helper holds the cooldown; a refusal is
  decided afresh from findings at most five seconds old, and `Busy` is not
  kept (`only_a_run_that_reached_the_helper_holds_the_cooldown`,
  `a_refusal_is_decided_afresh_from_recent_findings`).
- The power-off reads its whole world again right before it
  (`a_transaction_started_while_people_are_told_stops_the_power_off`).
- A lingering account blocks the power-off, and every systemd login class
  counts (`lingering_accounts_and_every_login_class_count`).
- A kernel that cannot load the compressed file is refused before the
  helper with a sentence on the page; the kernel's own
  `/sys/module/compression` decides when it exists
  (`a_module_the_kernel_cannot_load_is_refused_before_the_helper`,
  `the_kernel_says_which_compressed_modules_it_loads`). `wait_with_deadline`
  kills and reaps on a failed `try_wait` too (no test: that error cannot
  be made happen to a real child from a test).

The `*-power-off-chosen.png` screenshots were taken again with the new copy.

### Left: measured on the owner's machine before and after

This needs the release that carries the above installed (not an
unreleased build: AGENTS.md), and a real occurrence. Turn on both fixes in
Better Manager → Fixes, then, after the next sleep that shows a fault, look
for these lines in `journalctl -b`:

- `ucsi_acpi USBC000:00: failed to re-enable notifications (-110)`, then
  within about 30 s from `better-manager-daemon`: `usb-recovery: ucsi_acpi
  reloaded` and `usb-recovery: reload result Ucsi { ports: 2, expected_ports:
  2, charger: Online }` (or what did not come back);
- `xhci_hcd 0000:63:00.4: HC died; cleaning up`, then either `usb-recovery:
  controller rebound 0000:63:00.4` or `usb-recovery: after sleep: … rebind:
  Some(Err(UsbRefused(RestartNeeded)))` and, after the second failed sleep,
  the desktop notification "This PC can't go to sleep right now";
- with power-off chosen and the lid closed, logind's `System is powering
  down` instead of a night of `PM: suspend entry`.

Before (today's behaviour, from the journal): the faster port stops
charging until a reboot or a manual `modprobe -r ucsi_acpi && modprobe
ucsi_acpi`; after an `HC died`, every sleep fails all night. After: the
ports come back within about 30 s of a resume, and a blocked night is one
notification (or one power-off). Record both in this ticket, then move it to
`done/`.

Not verified by anyone yet (ADR 0046's consequences): a real
`finit_module` through the helper's sandbox, a real unbind and bind, the
notification as the person, the logind calls (`Docked` included), whether
apt holds a logind inhibitor while dpkg runs, and the post hook after a
failed attempt (the hook also queues the run from its `pre` call).

