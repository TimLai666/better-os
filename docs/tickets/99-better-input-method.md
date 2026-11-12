# Ticket 99 — Better Input: Zhuyin that needs no switching, and voice input

**Blocked by:** 98 for the NPU parts · **Status:** ready (design first)

## Why

The owner types Chinese with ibus-chewing and wants an input method like
ASUS's smart IME: no switching between Chinese and English, smarter word
choice and correction, and voice input, using the NPU where that pays off.
Of the two neural Zhuyin projects the owner named, 善解 (shanjie) is macOS
only and its upstream repository has disappeared; 靈犀注音 (0c-ime,
`0c-biggo/0c-ime`, read 2026-11-13) runs on Linux under IBus and Fcitx5 and
already has mixed Chinese–English typing (beta), fuzzy Zhuyin, toneless
input, Tab completion, and local personalisation. Its code is
AGPL-3.0-only (Go), its data CC BY-SA 4.0, and its neural models MIT, shipped
separately from the source.

## What to deliver first: a design

- Whether Better Input is a fork of 0c-ime or a front end that runs 0c-ime as
  is and adds voice input beside it. A fork carries AGPL-3.0-only obligations
  (source offer for network use, notices) into Better OS, whose own code is
  GPL-3.0-or-later; GPLv3 section 13 allows the combination, but the ADR must
  say how the two licences sit together and what is shipped where.
- Voice input: push-to-talk and a toggle key, streaming text into the same
  composition so it can be corrected by typing, punctuation, mixed Chinese and
  English, entirely on the machine. Candidate models: Whisper (MIT) and
  MediaTek's Breeze-ASR (Taiwanese Mandarin, check its licence), on the NPU
  when ticket 98's numbers say so, on the CPU otherwise.
- The owner's voice key. On the ASUS Vivobook S 14 M5406WA the voice key
  is Fn+F10 (the keyboard sends plain F1–F12 without Fn). It arrives on the "Asus WMI hotkeys" device as an
  unknown key: the kernel logs `asus_wmi: Unknown key code 0xe7` and reports
  `KEY_UNKNOWN` with that scan code (read 2026-11-13). A hwdb remap cannot
  reach it, because `sparse_keymap` only remaps codes already in its table, so
  Better Input listens for that scan code on that device itself, and the
  design also proposes the one-line upstream patch that maps 0xe7 in
  `asus-nb-wmi` to `KEY_VOICECOMMAND`. Press once to start dictation, again to
  stop; holding it to talk is a setting.
- The speech model is Taiwanese Mandarin: MediaTek's Breeze-ASR-26
  (Apache-2.0, fine-tuned from Whisper large-v2, about 1.5B parameters, read
  2026-11-13), the one place in this ticket where the NPU is expected to pay
  off; ticket 98 measures it.
- How it is installed and configured by Better Manager: the IBus engine set
  as the input source, the mixed-language mode on by default, ibus-chewing
  restored on removal.
- The control center page (ticket 89's neighbour) with its settings.
- The implementation tickets that follow.
