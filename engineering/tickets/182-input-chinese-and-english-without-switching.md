# Ticket 182 — Typing Chinese and English without switching

**Blocked by:** 180, 181

## Why

"No switching" is the first thing the owner asked for (ticket 99), and spec
section 3.2 makes it Better Input's default. No engine that Better OS can use
does it, so Better OS writes it.

## What to build

- **The decision** in `input-core`, as keys arrive (spec section 3.2): a run that
  is not Zhuyin in the person's layout is English; a run that is both is scored
  both ways (libchewing and the reranker for Chinese; an English lexicon and the
  person's English words for English); Shift+letter is English; digits after
  English stay digits; the losing reading is the first alternative under ↓, and
  choosing it is learned for that context.
- **The English lexicon**: choose a permissively licensed word list, record its
  licence in the third-party notices, and keep it in the package (no download).
- **Keys**: a single Shift tap toggles English-only mode (left, right, either, or
  none, from the setting); Shift with another key, held over 300 ms, or while
  Alt is held is not a tap, so ticket 94's Alt+Shift still switches sources; in
  mixed mode Space is a space and choosing is ↓ or the selection keys.
- **Space between Chinese and English** on commit (setting, off by default).
- **Toneless and mixed exclude each other** (spec section 3.2): turning one on
  turns the other off; the setting model reports which one disabled the other
  for the page's caption.
- **Evaluation**: a frozen mixed-text set (sentences with English words, model
  numbers, versions, decimals, times, single Zhuyin symbols, the cases in 0c's
  changelog) with the share of runs read correctly, added to
  `engineering/performance/input-performance.md`.

## Done when

- [ ] Unit tests for each rule above and for every case class in the
      evaluation set (`python`, `iPhone`, `DNS`, `win10`, `3.4`, `04:10`,
      `53,082`, `OK嗎`, `用Python寫`, `我ㄉ天`), including ↓ offering the other
      reading and learning it.
- [ ] The Shift tap rule tested with Alt held, with another key, and held 300 ms.
- [ ] The evaluation results are in the performance file with the command.
- [ ] fmt, clippy on stable, the crate's tests, and the license inventory check
      pass.
