# Ticket 188 — Hearing the voice key without a keylogger

**Blocked by:** the owner's decision D5 in `design/better-input-spec.md`

## Why

On the owner's ASUS Vivobook S 14 M5406WA, Fn+F10 arrives on the "Asus WMI
hotkeys" device as `KEY_UNKNOWN` with scan code 0xE7, which hwdb cannot remap
(spec section 5.1). Spec section 5.3 describes a system helper that opens only
that node and reports only that key; AGENTS.md and ADR 0030 keep every input
node out of the person's session.

## What to build

- **First, with the owner**: confirm what one press of Fn+F10 produces on that
  node (`MSC_SCAN 0xe7`, `KEY_UNKNOWN 1`, `KEY_UNKNOWN 0` in one report, and
  nothing on release), either with the owner running `evtest` or with a test
  build of the reader given that node read-only in a container. Record it in
  Notes.
- **Crate `input-voicekey`** (GPUI-free): judges a device by sysfs name, phys,
  and the DMI product against a closed table (one row today, spec section 5.3),
  and filters an event stream to "pressed" (0xE7 then `KEY_UNKNOWN 1`, or
  `KEY_VOICECOMMAND 1`), dropping everything else unread beyond the filter, with
  a 300 ms debounce.
- **`better-input-voicekey@.service`** started by
  `91-better-input-voicekey.rules` for that device only: `DevicePolicy=closed`
  and one `DeviceAllow=` for the node, `PrivateNetwork=yes`,
  `RestrictAddressFamilies=AF_UNIX`, open with `O_NOFOLLOW` and an input-major
  check, drop to `_better-input-voicekey` with no capabilities, a seccomp filter
  that refuses `EVIOCGRAB` and every uinput call; emits
  `org.betteros.VoiceKey1.Pressed` with no argument on the system bus, with a
  bus policy file.
- **The security review** `engineering/specs/voice-key-security.md` in the form
  of `touchpad-input-passthrough-security.md`: threat model, who holds which
  privilege, the layers that keep it to one node, that it never reads a typed
  key, that it never takes the hotkeys from the desktop, and what is not proven.
- **An ADR** for the boundary (a root-started unit reading one input node),
  amending nothing of ADR 0030 and pointing at the review.
- The page's voice-key test (ticket 190) uses `Pressed` through the voice
  service.

## Done when

- [ ] The press is recorded in Notes as seen on the device.
- [ ] Unit tests: device judgement (the hotkeys node accepted; the keyboard, a
      touchpad, a device with the right name but other phys, another DMI product
      refused), the filter over recorded streams (0xE7, other scan codes,
      `KEY_VOICECOMMAND`, a burst), the debounce.
- [ ] `verify-deb.sh` checks the rule, the unit's one `DeviceAllow=`, the
      account, and the bus policy; the seccomp filter test shows `EVIOCGRAB`
      refused.
- [ ] fmt, clippy on stable, the crate's tests pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

Nothing changes: the helper is a system service reading one evdev node and is the same under any session.
