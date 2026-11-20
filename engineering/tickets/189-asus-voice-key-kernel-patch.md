# Ticket 189 — Send the asus-nb-wmi patch that names the voice key

**Blocked by:** the owner (decision D7 in `design/better-input-spec.md`): only
the owner can load a test kernel module on the laptop and sign off a kernel
patch

## Why

Fn+F10 on the M5406WA sends ASUS WMI event 0xE7, which Linux master
(2026-11-20) does not map, so every Linux desktop sees an unknown key. A
one-line keymap entry fixes it for everyone (spec section 5.6).

## What to do

- Regenerate the patch in spec section 5.6 against the pdx86 `for-next`
  branch with `git format-patch`.
- The owner builds `asus-nb-wmi` with it for the running kernel, loads it on the
  M5406WA, and confirms with `evtest` that Fn+F10 reports `KEY_VOICECOMMAND`
  and that the other ASUS hotkeys behave as before; then reloads the stock
  module.
- The owner sends it with `git send-email` to the maintainers
  `scripts/get_maintainer.pl` names for the file (the ASUS WMI and X86 platform
  driver entries in `MAINTAINERS`), copying platform-driver-x86@vger.kernel.org,
  signed off under the Developer's Certificate of Origin, and answers review.
- When it is merged, record the commit and the first kernel release in this
  ticket and in spec section 5.6. Ticket 188's helper already accepts both
  forms; nothing else changes.

## Done when

- [ ] The test on the laptop is recorded in Notes.
- [ ] The patch is sent; its lore.kernel.org link is in Notes.
- [ ] The merge (or the maintainers' answer) is recorded.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

Nothing changes.
