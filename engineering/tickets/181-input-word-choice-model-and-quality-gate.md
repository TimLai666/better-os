# Ticket 181 — Word choice from context, punctuation on Enter, and the quality gate

**Blocked by:** 180

## Why

"Smarter word choice" is the reason for Better Input (ticket 99). Spec section
2.3 (option C) reranks libchewing's N-best with 0c's MIT word-choice model, and
section 3.7 says Better Input replaces 新酷音 only when it measurably chooses
better. This ticket builds the model path and the measurement that decides it.

## What to build

- **`better-input-lm`**, a child process of the engine speaking a typed
  protocol over stdin and stdout, opening no socket: loads
  `Funmula/0c-sequence-d512` (and `0c-punct-d512`) through `ort` with ONNX
  Runtime's library shipped in the package (not in Ubuntu's archive; MIT notice
  added), builds the input tensors exactly as the model card documents, scores up
  to 16 candidates of 2–32 syllables with up to 64 characters of context, and
  returns scores. A reply later than 100 ms, a crash, or a candidate with a
  character outside the model's vocabulary falls back to libchewing's order for
  that case; the engine restarts the worker at most three times a minute.
- **Fusion**: engine score + w × model score with the override margin τ,
  recalibrated on libchewing's score scale (the card's w = 2, τ = 0 are for 0c's
  engine), the calibration script and its data recorded.
- **Punctuation on Enter** (spec section 7.2, off by default) and the punctuation
  call ticket 186 uses for dictation, through `0c-punct-d512`, inserting ，。？！、；
  between Han characters without changing a character.
- **Model files**: pinned Hugging Face revisions and the SHA-256 of every file,
  verified before use, stored as spec section 4.9 says; the download itself is
  ticket 186's model manager (one downloader for every model).
- **The quality gate** (spec section 3.7): a frozen test set of Taiwanese
  Traditional Chinese sentences from a licence-clean corpus, readings
  synthesised from the lexicon, a fixed share of hard homophone cases, frozen
  before any scoring; first-choice accuracy for libchewing alone, Better Input
  without and with the model, and 0c-ime 0.2.13 statistical and neural run
  unmodified in a container as a measuring instrument; per-key latency P50/P95
  and the worker's resident memory. Results in
  `engineering/performance/input-performance.md` with the hardware. The gate
  value (beats libchewing alone) is exported for ticket 185's install step.

## Done when

- [ ] Unit tests for the tensor layout against the model card's worked examples
      (the card's `rank` and `choose` examples give the same order), the
      fallback on timeout, crash, and out-of-vocabulary character, and the
      restart limit.
- [ ] The test set, its freeze record, and the results table are in the
      repository; the numbers in the table come from a run, with the command.
- [ ] fmt, clippy on stable, the crates' tests, and the license inventory check
      pass.

## In the Better OS session (2026-11-21, ADR 0038 option D, decision C4)

Nothing changes: word choice and the quality gate are in the engine core. The gate decides the default input source in both sessions.
