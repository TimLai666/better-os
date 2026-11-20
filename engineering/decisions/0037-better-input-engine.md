# ADR 0037: Better Input's engine is Rust on libchewing, with 0c's MIT models

## Status

Proposed on 2026-11-21 for ticket 99, for the owner to accept or refuse
(decision D1 in `design/better-input-spec.md`). Nothing it describes is built
until it is answered; tickets 180 to 183 and 186 to 187 are blocked on it.

## Context

The owner types Chinese with ibus-chewing (新酷音) and asked for an input method
like ASUS's: no switching between Chinese and English, smarter word choice, and
voice input on the machine, with dictated text landing in the same composition
so it can be corrected by typing (ticket 99). The design is
`design/better-input-spec.md`; section 2 has the evidence summarised here, all
read on 2026-11-21.

Of the open Zhuyin engines, only 0c-ime (靈犀注音, `0c-biggo/0c-ime`) has both
halves today: mixed Chinese–English typing (in beta, off by default) and neural
word choice (86% first choice correct on its own frozen test, against 64.2% for
新酷音 0.11.0 and 67.2% for 小麥注音). It is AGPL-3.0-only Go, with CC BY-SA 4.0
data, MIT models published separately, a Python neural worker, a Tauri settings
program, and a C++ Fcitx5 shell. It has made 14 releases in 31 days, all marked
public-beta. libchewing, the engine of 新酷音, has been rewritten in Rust
(LGPL-2.1-or-later), and since 0.9 has fuzzy and toneless conversion; Ubuntu 24.04
ships 0.6.0. McBopomofo exists on Linux only for Fcitx5; Rime's bopomofo schemas
do not switch automatically either.

Better OS's constraints: first-party code is GPL-3.0-or-later (ADR 0003); Go is
allowed "only when networking, remote management, agents, or standalone service
tooling has a concrete advantage", and Python, JavaScript and Tauri are not
allowed in production components (`engineering/specs/language-policy.md`);
everything is installed and removed through Better Manager in one action
(AGENTS.md).

Voice text can reach an application only through the focused input context's
engine, as its own preedit and commit: IBus runs one engine per context, and the
brief rules out synthetic key events. So whoever owns the engine owns the voice
path.

## Options

**A. Fork 0c-ime.** Carry its Go engine and IBus shell, add an interface for
dictated text, replace the Python workers with a Rust worker on the same stdio
protocol and the Tauri program with the control center page, drop the other
shells, rebase on each upstream release.

**B. Package upstream unmodified and put voice beside it.** Typing as 0c has it
(the statistical engine only, unless a Python runtime is bundled, which the
language policy forbids); voice text cannot enter 0c's composition because 0c's
engine takes no text from another process.

**C. A Rust engine on an existing language model.** `better-input-engine` on the
`chewing` crate for Zhuyin layouts, conversion, fuzzy and toneless modes and the
user dictionary; 0c's MIT `0c-sequence-d512` model as a reranker over
libchewing's N-best (its card documents the inputs and says the fusion weights
are recalibrated for another engine); 0c's MIT `0c-punct-d512` for punctuation;
mixed Chinese–English typing written by Better OS.

## How the licences sit together

- **A and B ship AGPL-3.0-only code beside GPL-3.0-or-later code.** GPLv3
  section 13 permits linking or combining a GPLv3 work with an AGPLv3 work and
  conveying the result; each part keeps its own licence, and AGPLv3 section 13's
  network requirement applies to the combination. Better OS's code is used under
  version 3 for that. Kept as separate programs over pipes and D-Bus, the two are
  an aggregate and section 13 is not needed at all.
- **AGPLv3 section 13 itself** obliges a *modified* version to offer its source
  to users who interact with it remotely over a network. A local input method has
  no such users. A fork (A) must therefore never let any part serve a network
  user (0c has a local vocabulary API) without adding that offer; B is unmodified
  and owes nothing under section 13.
- **Object code**: every `.deb` that carries 0c's programs must come with their
  Corresponding Source under GPLv3/AGPLv3 section 6; for A and B that is 0c's
  tagged source (and, for A, Better OS's changes and build scripts) published at
  the same place as the release assets (section 6d).
- **CC BY-SA 4.0 data** (0c's compiled lexicon, which A and B ship): attribution
  to Funmula Co., Limited, the licence URI, and a note of changes; a dictionary
  Better OS changes is itself CC BY-SA 4.0.
- **MIT models** (all three options): downloaded on install, never inside a
  `.deb`, with the MIT notice beside the files and in the third-party notices.
- **C** ships no AGPL code. libchewing is LGPL-2.1-or-later, whose section 3 lets
  a GPL program carry it; its data is LGPL-2.1-or-later (Debian's copyright file);
  whisper.cpp and ONNX Runtime are MIT.
- **Notices tooling**: `packaging/generate-third-party-notices.sh` reads
  `Cargo.lock` only. A and B need a second inventory for Go modules (`go.sum`).

## What it means for the language rule

A and B make a Go input method part of Better OS. The language policy would
have to be widened by this ADR for an input method, which is none of the cases
it allows Go for, and 0c's Python and Tauri parts would still have to be
replaced (A) or left out (B). A also needs a Go 1.26 toolchain in the release
build environment (ADR 0021): Ubuntu 24.04's archive stops at Go 1.24. C stays
inside the rule; whisper.cpp and ONNX Runtime are C/C++ libraries behind Rust
bindings, which the policy accepts as dependencies.

## Decision (proposed)

**C.** Better Input's engine is Rust on libchewing, with 0c's MIT models for
word choice and punctuation, and Better OS's own mixed-typing logic. It is the
only option that meets every part of ticket 99, including voice into the same
composition, without depending on a third party accepting a change; it keeps
Better OS in one language and one licence; and because libchewing is 新酷音's
engine, the person's existing words and settings carry over.

0c-ime is used as a measuring instrument only: ticket 181 runs release 0.2.13,
unmodified, in a container on the same frozen test set, so the quality bar is
0c's, not a guess.

## Consequences

- Tickets 180–183 build the engine, the reranker and its quality gate, mixed
  typing, and the IBus engine process; 184–191 are the same in every option
  except that A would change 180–183 into fork work.
- Better Input does not replace 新酷音 as the input source until ticket 181's
  gate shows it chooses words better than libchewing alone; 0c's numbers on the
  same set are reported beside it.
- Mixed Chinese–English typing is Better OS's own heuristics and scoring, and the
  longest-lived risk of this choice: 0c's changelog shows how many cases it took
  them (model numbers, decimals, single Zhuyin symbols), and ticket 182 carries
  those cases as tests.
- ONNX Runtime is not in Ubuntu's archive, so its library ships inside the
  `better-input` package with its MIT notice.
- The reranker was trained on candidates from 0c's dictionary; its fusion weights
  (w = 2, τ = 0 on 0c's scale) are recalibrated on libchewing's scores, and
  candidates with characters outside its 27,857-character vocabulary fall back to
  libchewing's order, as its card prescribes.

## If the owner chooses A instead

This record is replaced by one that widens the language policy for a Go input
method with the concrete reason (reusing 0c's engine), names the fork's
repository and rebase rule, and adds the Go notices generator and the Go
toolchain to ADR 0021's environment. Tickets 180–183 are rewritten as fork work;
184–191 stand.

## Alternatives not taken

- **B**, because voice text cannot enter 0c's composition, and the neural path
  needs Python.
- **ibus-rime with bopomofo schemas, or fcitx5-mcbopomofo**: neither switches
  between Chinese and English by itself or chooses words from context, and
  McBopomofo would need Fcitx5 in place of GNOME's IBus.
- **Waiting for 0c to accept an interface for external text**: possible to
  propose, impossible to plan on.
