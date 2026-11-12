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
