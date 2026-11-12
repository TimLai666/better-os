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
- How it is installed and configured by Better Manager: the IBus engine set
  as the input source, the mixed-language mode on by default, ibus-chewing
  restored on removal.
- The control center page (ticket 89's neighbour) with its settings.
- The implementation tickets that follow.
