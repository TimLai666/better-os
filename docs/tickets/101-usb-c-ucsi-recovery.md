# Ticket 101 — The USB-C ports keep working on Linux

**Blocked by:** none for part 1; the owner's go-ahead on Secure Boot for part 2 · **Status:** ready

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

## Part 2 — a real fix in the driver

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
