# Ticket 99 — Better Input: Zhuyin that needs no switching, and voice input

**Blocked by:** 98 for the NPU parts

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

## What was built

The design, in the form of tickets 83 and 95; nothing was written to the
machine.

- **The spec**, `design/better-input-spec.md`: the owner's nine decisions first
  (D1 the engine, D2 the speech model, D3 where its file comes from, D4 when
  models download, D5 how the voice key is read, D6 the hold-to-talk key, D7 the
  kernel patch, D8 the input-source list, D9 a fourth update mechanism in
  AGENTS.md), each with a recommendation and its costs; every source read on the
  machine and upstream on 2026-11-21; the processes and their privileges; 0c-ime
  as it is today against libchewing, McBopomofo and Rime; the three engine
  options with their work and licence consequences (GPLv3/AGPLv3 section 13,
  section 6 source offer, CC BY-SA 4.0 data, MIT models) and the language rule;
  typing (composition, mixed Chinese and English, fuzzy, toneless, Tab, learning,
  the carry-over from 新酷音, the quality gate); voice (modes, the audio and text
  path, punctuation, model, runtimes with licences, others' latency numbers on
  similar chips, the NPU, privacy, model download, storage and removal); the
  voice key (what the kernel sends, the helper, the upstream patch, checked to
  apply to Linux master `d0f871bf`); install, turn off, update, remove in one
  action each, including why IBus must restart once; the control center page
  with every card and state in zh-TW and English; what is not verified and which
  ticket owns it.
- **ADR 0037** (proposed), `engineering/decisions/0037-better-input-engine.md`:
  recommends a Rust engine on libchewing with 0c's MIT models (option C), and
  says what changes if the owner picks the fork.
- **Mockups**, `design/better-input/`: the page, its voice group while models
  download, dictation in a text field, and typing without switching, light and
  dark, in ticket 83's technique and tokens.
- **Tickets 180–191** (161 and 170 to 175 were taken on `main` while this was
  written):
  engine core; word choice and the quality gate; mixed typing; the IBus engine;
  the Better Defaults input-source adapter (unblocked, any engine); the
  component's install and removal; the voice service; dictation in the
  composition; the voice-key helper with its security review; the kernel patch
  (the owner's); the control center page; the GPU and the NPU (after ticket 98).

Corrections to this ticket's own text, found while checking it:

- **Breeze-ASR-26 is not a Taiwanese Mandarin model.** Its card describes
  "BreezeASR-Taigi": Taiwanese Hokkien speech written as Mandarin characters.
  The Taiwanese Mandarin model with code-switching is Breeze-ASR-25 (Apache-2.0,
  Whisper large-v2's shape, 1.55 B parameters; the research note's "about 2B" does
  not match its `config.json`).
- **0c-ime's mixed typing is off by default** and excludes toneless input; its
  repository was created on 2026-10-19 and every release so far is public-beta.
- **whisper.cpp's Ryzen AI NPU path is no longer Windows-only**: its README lists
  Ubuntu 24.04 since the VitisAI plugin merged on 2026-08-24, still with AMD's
  proprietary runtime and caches only for OpenAI's Whisper models.
- **Hold-to-talk cannot use Fn+F10**: the kernel reports every ASUS WMI key
  autoreleased, so the release is never seen; the design uses Right Alt.

Not done or not verified (each owned by a ticket, spec section 9):

- No latency, memory, or accuracy was measured on the owner's machine; the
  speech numbers are others', on an HX 375 under WSL and an 8845HS.
- What Fn+F10 produces on `event8` was read from the kernel source, not from the
  device; the kernel's "Unknown key code 0xe7" line was not in this boot's
  journal as the user can read it.
- Whether `ibus-daemon` restarts an exited engine by itself, whether Mutter 46's
  text-input-v3 is version 2, and how Ubuntu 22.04 restarts IBus.
- OpenBenchmarking.org and Phoronix refused the request, so their HX 370
  whisper.cpp results were not read.
- The mockups are zh-TW only, as tickets 83's and 95's were; the English copy is
  in the spec. GNOME Shell draws the candidate popup, so the mockups show its
  position and text, not its exact look.

### Screenshots

In `/home/tim/Developer/better-os-evidence/99/`: every mockup rendered by
headless Chrome, one PNG per HTML file (`page`, `voice-setup`, `dictation`,
`typing`, each light and dark).
