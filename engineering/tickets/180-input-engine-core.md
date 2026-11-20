# Ticket 180 — Better Input's engine core: composition, conversion, learning

**Blocked by:** the owner accepting ADR 0037 with option C (decision D1 in
`design/better-input-spec.md` section 0)

## Why

`design/better-input-spec.md` section 3 describes a Zhuyin engine that needs no
switching and learns from the person, and section 4 needs that engine's
composition to take dictated text. This ticket builds the part with no IBus and
no model: the composition, Zhuyin conversion on libchewing, learning, the
person's words, settings, and the import from 新酷音, so tickets 181–183 and 187
each add one part.

## What to build

- **Crate `input-core`** (no GPUI, no IBus, no D-Bus): the composition as a list
  of segments (typed Zhuyin with reading and choice, typed English, dictated text
  with a stable or tentative flag), every editing key of spec section 3.1 over
  every segment kind, commit and clear rules (an unfinished syllable is cleared,
  not committed, on focus loss), the 64-character committed context per input
  context, the password/PIN rule (nothing learned, nothing sent to a model).
- **Conversion on the `chewing` crate** (libchewing, LGPL-2.1-or-later): pin the
  version (0.13.x, because 0.14 drops the sqlite migration spec section 3.6
  needs), the layouts and their zh-TW and English names for the page (confirm
  the list in spec section 7.2 against the crate), candidates and paging,
  selection keys, fuzzy pairs (spec section 3.3) with the 近音 comment, toneless
  mode, and an N-best list with scores for ticket 181.
- **Tab completion** (spec section 3.4) from the lexicon and the person's words
  only, shown only when the best continuation clearly leads.
- **Learning and My words** (spec section 3.5): the user dictionary, "Learn from
  what I choose", Shift+←/→ then Enter to add, Shift+Delete to forget a learned
  entry, list/add/remove/export/import as a typed API for ticket 190's page,
  "Clear what Better Input learned" keeping the person's own words.
- **Settings**: one versioned JSON file in `~/.config/better-os/input/`, every
  setting of spec section 7.2–7.4 with its default, written atomically, a newer
  file read-only (as other Better OS settings stores do).
- **Import from 新酷音** (spec section 3.6), once, on first run: settings read
  from `org.freedesktop.IBus.Chewing` through a trait (tests use a fake), learned
  phrases copied from `~/.chewing/chewing.sqlite3` with libchewing's reader;
  新酷音's own files and settings never written. The page's "Imported {n} words"
  comes from here.
- Storage paths `~/.local/share/better-os/input/user/` and
  `~/.config/better-os/input/` exactly as spec section 4.9 names them.

## Done when

- [ ] Unit tests for every editing key on every segment kind, commit and clear
      on focus loss, the password rule, fuzzy and toneless candidates, Tab
      completion offered and not offered, learning on and off, My words
      add/remove/export/import round trip, settings defaults and a newer file.
- [ ] The import reads a fixture `chewing.sqlite3` made by libchewing 0.6 and a
      fake gsettings, produces the expected words and settings, and a test
      asserts the source files are byte-identical afterwards.
- [ ] fmt, clippy on stable, the crate's tests, and the license inventory check
      (the lockfile gains `chewing`) pass.
