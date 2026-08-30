# Better Files preview and search policy

The decisions ticket 35 had to make about `files-preview` and `files-search`,
and the limits of each. Issue #6 leaves all of these open or asks for an
explicit policy rather than a silent choice.

## Preview: what the security boundary actually is

Issue #6 requires untrusted file parsers to be treated as a security boundary. A
preview pane reads a file that arrived from somewhere else — a USB stick, a
download, an email attachment — and hands it to a decoder. Four things enforce
the boundary, and it is worth being exact about what each one does and does not
buy.

| Mechanism | What it prevents | What it does not |
| --- | --- | --- |
| `PreviewLimits::max_source_bytes` (64 MiB) applied by the engine before any provider is called | A parser ever seeing a file larger than policy allows | Anything about a small malicious file |
| `image::Limits` — `max_image_width`, `max_image_height`, `max_alloc` — handed to the reader before `decode` | A header claiming enormous dimensions turning into an enormous allocation | A decoder bug that is not an allocation bug |
| Dimensions read from the header first, in a separate pass | Allocating at all for an image the policy would refuse | A file whose header lies about a body that then overruns |
| `catch_unwind` around every provider call | A panicking decoder taking the file manager down; the preview degrades to metadata instead | Memory corruption, which is not a panic |

**This is a boundary, not a sandbox.** A real sandbox means the decode running
in a separate process with a restricted seccomp profile and no filesystem
access, so that a memory-safety bug in a decoder cannot become anything at all.
That is a design with a process-spawn cost per preview and an IPC protocol, and
it is the right shape once there are more formats than image and text. It is
recorded here as the follow-up, not implemented.

`image` is compiled with `default-features = false` and exactly four decoders —
PNG, JPEG, GIF, BMP. Every one of them is already in the workspace lockfile, so
this narrows the attack surface without adding a package. The formats that are
absent are absent on purpose: EXR, AVIF, and TIFF are large parsers for formats
a file manager rarely meets.

## Preview: degrade, never blank

Every refusal produces a `Preview::Metadata` carrying a `DegradeReason` with a
stable key. There is no path that returns nothing, and no path that shows an
empty pane. The reasons are distinct because they need different words:

- **`NoProvider`** — nothing in this build renders the type.
- **`TooLarge`** — carries the limit, so the pane can state it rather than
  saying "too large" and leaving the user guessing.
- **`Binary`** — a text provider found a NUL byte in the first chunk.
- **`DecodeFailed`** — the parser ran and refused. A `.png` that is not a PNG
  lands here rather than being misread.
- **`ParserFaulted`** — the parser panicked and the boundary caught it.
- **`Unreadable`** — the file could not be opened at all.

## Preview: newest wins, and nothing else runs

One worker thread, one request at a time, newest request cancels the previous
one. Holding Down through a folder of photographs produces sixty requests and
the user wants the sixtieth; a queue would spend the thread on fifty-nine
previews nobody will see, and a pool would spend fifty-nine threads on them.

Every outcome carries the id it was asked under, and the pane discards anything
that is not the current id. Without that, a slow decode of the previous
selection would replace the preview of the current one.

## Preview: encoding detection is evidence, not statistics

A byte-order mark is evidence. UTF-8 that decodes is evidence. Everything else
that is not binary is shown as Latin-1 — the one single-byte mapping that never
fails and never invents a character — and the pane says which reading it is
showing. There is no statistical charset guesser, because a guesser that is
right nine times in ten silently corrupts the tenth file and gives the user no
way to tell.

A UTF-8 file cut mid-character by the bounded read stays UTF-8: the incomplete
trailing sequence is dropped rather than the file being declared Latin-1 on the
strength of a byte that was never a whole character.

## Preview: what a folder summary counts

The immediate children only, up to `max_folder_entries` (20,000), and it says
when the limit stopped it. A recursive size is a filesystem walk with no bound
in the direction that matters, and a truncated count reported as a total would
be a number that looks exact and is not.

## Preview: what has no preview

An application row and a trashed item both answer `None` from
`Entry::as_local_path`, so both reach the pane as "there is no file behind this
item". For an application that is correct and final: the location exists
precisely so that no `.desktop` file is presented as the application.

For a trashed item it is a gap. The stored path is a real file and previewing
before restoring is exactly what a person wants from the Trash, but
`files-core` deliberately does not expose a trashed entry's stored path as the
entry's path. Closing the gap means a typed accessor for it, which is a
`files-core` change and a follow-up.

## Search: what "current location" means, and what it does not

`CurrentDirectoryProvider` is fed the entries the pane already holds. Searching
where you already are therefore costs no I/O at all — it is a scan over a list
in memory. The consequences are worth stating:

- A search only sees what has been listed. On a directory that is still
  streaming, results keep arriving as entries do.
- Subdirectories are not searched. `SearchScope::Recursive` is representable so
  the UI can offer the choice; no provider claims it yet.
- `SearchScope::Indexed` is representable for the same reason. The indexed
  engine is Issue #6's deferred decision and needs an ADR.

## Search: the hidden rule is the search's own

Issue #6 requires hidden files to follow an explicit *search* setting rather
than inheriting the view's. That is why `DirectoryModel::iter_all` exists: a
search that could only see the visible projection could not implement the
setting at all. Searching for a dotfile finds it with the setting on, without
the view changing.

## Search: incremental, in slices the caller sizes

A run is advanced 4,096 entries per frame. On a 100,000-entry directory that is
tens of microseconds of work per frame, spread over about a second, with every
intermediate result already in final order — `CurrentDirectoryRun` inserts each
hit at its sorted position rather than sorting a partial list that is about to
change. Typing restarts the run and does nothing else, so a keystroke costs an
allocation.

## Search: ordering is decided, not emergent

Match kind dominates: exact, then prefix, then substring, then fuzzy
subsequence. Ties break on shorter name, then natural name order. A search whose
order changes between two identical runs is a search you cannot use twice, so
there is no scoring heuristic with a tunable weight anywhere in it.

## Search: what is not wired

Keyboard movement through search results uses the model's visible list, so a hit
the view is currently hiding — a dotfile found with the search setting on while
the view is off — has no place in the keyboard cursor. It can be clicked and
opened; it cannot be arrowed to. Fixing that means the selection working over
the result list rather than the model's, which is a content-area change rather
than a search one.
