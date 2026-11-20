# Ticket 186 — The voice service: capture, recognition, punctuation, models

**Blocked by:** 180, and the owner's decisions D2, D3, and D4 in
`design/better-input-spec.md`

## Why

Voice input on the machine (ticket 99). Spec section 4 says how audio is
captured, recognised, punctuated, and handed to the engine, which model and
runtime, and how the models are downloaded, stored, and removed.

## What to build

- **First, measure** on the owner's CPU in a container (a CPU benchmark needs no
  host change; Chefer per AGENTS.md): whisper.cpp through `whisper-rs` with the
  model of D2 converted as D3 says, at q5_k and q8_0, for 2, 5, 10, and 20
  seconds of speech, with `audio_ctx` sized to the audio and at full context,
  at 8, 12, and 24 threads: wall time, CPU-seconds, resident memory, and load
  time. Write the table into `engineering/performance/input-performance.md`.
  Apply spec section 4.6's rule (final pass for 5 s over 3 s on the CPU) and
  record what the default becomes.
- **`better-input-voice`**, a systemd user unit (enabled at install, AGENTS.md),
  `Nice=5`: one PipeWire capture stream at 16 kHz mono with `application.name`
  and `media.role`, opened only while listening (PulseAudio's simple API where
  PulseAudio is the server, Ubuntu 22.04); Silero VAD; the streaming policy of
  spec section 4.6 (partials every ~1.5 s with `audio_ctx` sized to the audio,
  a word stable after two agreeing runs, a final pass split at pauses, at most
  30 s per window) in the GPUI-free crate `input-voice-core`; the model loaded on
  the first dictation and unloaded after 5 minutes idle.
- **Text rules** (spec section 4.3): pause spaces between Han characters removed
  where punctuation goes, punctuation from ticket 181's call, spoken symbols as
  whole phrases between pauses, Traditional output with single-form
  Simplified characters converted from OpenCC's table (licence in the notices).
- **The interface to the engine** (spec section 4.2): `BeginDictation`,
  `Partial(serial, stable, tentative)`, `Final(serial, text, alternatives)`,
  `Cancel`, unicast to the engine checked by bus name and executable path;
  nothing logged; audio never written to disk.
- **The model manager**, one downloader for every model of spec section 4.9:
  pinned URLs and SHA-256s compiled in, resumable, verified before it is moved
  into place, a bad file deleted, NetworkManager's metered state respected,
  progress reported to Better Manager's Activity and the page, a feature turned
  off deleting its model.
- **Breeze conversion** (if D3 a): the conversion recipe (whisper.cpp's own
  converter run by a maintainer outside the product, from MediaTek's pinned
  revision), the asset's NOTICE and licence, and its publication with the
  release assets.

## Done when

- [ ] The measurement table is in the performance file, with the commands.
- [ ] Unit tests in `input-voice-core` for the streaming policy over recorded
      hypothesis sequences, the text rules, the spoken symbols, the model
      catalog's verification, deletion of a bad file, and the metered deferral.
- [ ] A private-bus test: a fake engine receives `Partial` and `Final` for a
      fixture WAV fed through a fake audio source; a second client never
      receives them.
- [ ] fmt, clippy on stable, the crates' tests, and the license inventory check
      pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

Nothing changes: capture is PipeWire's and the locked or inactive check is logind's in both sessions.
