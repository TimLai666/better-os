# Ticket 56 — A person can compress files and extract archives in Better Files

**Epic:** Better Files (Issue #6) · **User Story:** a person can make a `.zip`
or `.tar.gz` of a selection and extract one they downloaded, as a job they can
pause, cancel, and resume · **Branch:** `ticket-56` · **Blocked by:** 51, 52,
53, 54, 55 (they edit the same job engine and window) · **Status:** implemented, not merged

## What it delivers

Issue #6 lists archive and extract as job operations, and neither exists.
After this ticket both are `files-operations` job kinds with a menu entry in
the window.

## Acceptance criteria

- [x] Create `.zip`, `.tar`, `.tar.gz`, and `.tar.zst` from a selection.
- [x] Extract the same four formats into a new folder named after the archive.
- [x] Extraction refuses entries whose path escapes the target (absolute paths,
      `..`, symlinks pointing outside) and names the entry.
- [x] Extraction has limits on total unpacked size and entry count, and stops
      with a clear error when an archive exceeds them.
- [x] Both are jobs: progress, cancel, and resume behave like copy.
- [x] New dependencies are pure Rust where one exists, and
      `docs/third-party-licenses.md` is regenerated in the same commit that
      moves `Cargo.lock`.

## What was built

Compress and extract are two new `files-operations` job kinds,
`Operation::Archive` and `Operation::Extract`, and the window has a Compress…
and an Extract button for the selection.

- **One item per archive.** A compress job is one item, the archive file. Its
  progress total is the bytes of file content the planning walk counted, and it
  walks the sources again when it runs. An extract job is one item per archive,
  and its total is the archives' own size, because what they unpack to is not
  known until they are read. Pause and cancel land between chunks of whatever
  is being read, the way they do for a copy, and a failed item is retried whole.
  Nothing in the engine changed: the job record, the item journal from ticket
  55, and the `JobObserver` calls work as they do for any other kind.
- **Nothing half-made.** An archive is written under a temporary name beside
  its destination and renamed into place when the stream is complete. An
  extraction goes into a temporary directory beside the destination and is
  renamed to the archive's folder only once every entry is in. A refusal, a
  limit, a damaged archive, or a cancel removes the temporary. Neither kind
  claims rollback, because there is never a partial result to undo.
- **The folder.** `photos.tar.gz` is extracted into `photos/` beside it. The
  name decides the folder; the format is read from the archive's first bytes,
  and from the name only when they say nothing, which is the case for a tar
  older than POSIX. A folder, or an archive, that already exists is a conflict
  answered by the existing conflict policy: skip, rename to `photos (copy)`, or
  overwrite. Overwrite is refused for a folder, because replacing a directory
  would delete what is inside it without asking, the same rule a copy follows.
- **Hostile entries.** Every entry name is checked before anything is written
  for it, and a refusal stops the extraction with
  `files.operation.error.archive_entry_refused`, which carries the archive, the
  entry's name as the archive spells it, and a reason key: an absolute path,
  a `..` component, an unusable name, a parent directory that is a link, an
  entry replacing a directory or a link, a symbolic link resolving outside the
  folder, a hard link to anything outside it, or a hard link to something the
  archive has not extracted. A link's target is resolved the way the kernel
  would, against the links already extracted, and a `..` only counts when it
  climbs out of a directory that exists at that moment. That last rule is what
  stops `s -> .` followed by `e -> s/..`, which reads as inside and resolves
  outside, and `e -> p/..` followed by `p -> .`, which is the same trap with the
  entries in the other order. Nothing is written through a link and no link or
  directory is replaced, so a link checked once stays inside. Set-user-ID,
  set-group-ID, and sticky bits are dropped; device nodes, fifos, and sockets
  are skipped and logged.
- **Limits.** `policy::MAX_EXTRACTED_BYTES` (64 GiB written) and
  `policy::MAX_EXTRACTED_ENTRIES` (1,000,000 entries read), carried in
  `CopyPolicy::extract_limits` so the suite can prove them with a small
  archive. Bytes are counted as they are written, so an entry that understates
  its size is still stopped. A zip, which declares its entries and sizes up
  front, is refused before anything is written when the declarations already
  exceed a limit. Reaching one fails the item with
  `files.operation.error.archive_limit_exceeded`, naming the limit.
- **What goes into an archive.** Permission bits, modification times, and
  symbolic links as links. A zip stores local time with two-second resolution,
  converted through the C library's local time, because that is how every
  other zip tool reads it. A file that changes size while it is being read
  fails the archive with `externally_modified`, since a header states the size
  before the content. A zip cannot hold a name that is not UTF-8, and such a
  name fails the job naming the entry rather than being renamed. A tar keeps
  such names as bytes.
- **The window.** `commands::archive_actions` decides whether Compress and
  Extract are enabled: both need a folder to write into and a selection made
  entirely of real files and folders, and Extract needs every selected entry to
  be a file whose name is one of the four formats. Compress opens a chooser
  with one button per format, each labelled with the file it will make, named
  after the one selected item (`report.pdf` gives `report.zip`) or after the
  folder when several are selected. Both labels and the refusal have zh-TW and
  en-US text. The window has no selection menu, so the two are toolbar buttons
  rather than menu entries.
- **The keyboard.** `Ctrl+Shift+P` opens the format chooser for the selection
  and `Ctrl+Shift+E` extracts it, both through `keys::command_for` like every
  other shortcut. No file manager has a common key for either: Nautilus, Files
  on GNOME, binds none, and neither does Windows File Explorer. `C` and `A`
  were the obvious letters and are taken, because `Ctrl+C` and `Ctrl+A` are
  Copy and Select All with or without Shift, so Compress is `P` for "pack" and
  Extract is `E`. Both need Shift, so `Ctrl+P` and `Ctrl+E` stay unbound. Neither
  is a GNOME Shell, Mutter, or media-key default, and IBus's unicode and emoji
  keys are `Ctrl+Shift+U` and `Super+.`; the keybinding schemas on the Zorin 18
  host were read to confirm none of them binds either key. In the chooser,
  `keys::chooser_key_for` decides every key: Up, Left, and Shift+Tab move to
  the previous format, Down, Right, and Tab to the next, both wrapping; Enter
  or Space makes the archive in the focused format; Escape closes it. Nothing
  else reaches the file list while it is open. It opens focused on `.zip`, the
  format any recipient can open, and the focused format is drawn as the
  primary button. `commands::CompressChooser` holds the focus, with no GPUI in
  it. `tracking::written_paths` names the archive file for a compress
  and the destination folder for an extract, so a compress onto a removable
  disk or an extract onto one is reported to the storage layer as a write to
  that disk.

### Dependencies

All pure Rust, all compatible with GPL-3.0-or-later, and none with a
`rust-version` above 1.95.

| Crate | Version | License | rust-version | Why |
| --- | --- | --- | --- | --- |
| `zip` | 8.6.0 | MIT | 1.88 | Zip read and write. Default features off; only `deflate-flate2`, so no AES, bzip2, deflate64, LZMA, xz, PPMd, zopfli, or the C `zstd` |
| `tar` | 0.4.46 | MIT OR Apache-2.0 | 1.63 | Tar read and write. Default `xattr` feature off, since nothing is unpacked through the crate |
| `flate2` | 1.1.9 | MIT OR Apache-2.0 | 1.67 | Gzip, and zip's deflate. Default `miniz_oxide` backend; already in the lockfile |
| `ruzstd` | 0.9.0 | MIT | 1.87 | Zstd read and write |
| `twox-hash` | 2.1.4 | MIT | 1.81 | New, through `ruzstd`: the frame checksum |
| `typed-path` | 0.12.3 | MIT OR Apache-2.0 | 1.65 | New, through `zip` |

`miniz_oxide`, `crc32fast`, `adler2`, `indexmap`, `memchr`, `filetime`, and
`libc` were already in the lockfile. `docs/third-party-licenses.md` is
regenerated for the new `Cargo.lock`.

**Why `ruzstd` and not `zstd`.** The `zstd` crate binds the C library through
`zstd-sys`, which compiles it. `ruzstd` 0.9.0 decodes every zstd stream and
encodes at two levels, uncompressed and "fastest", which it describes as
roughly `zstd -1`; its better levels are declared and unimplemented. Fastest
is what Better Files uses, so a `.tar.zst` it writes is larger than one `zstd`
would write at its default level 3, and any zstd reader opens it. The encoder
also unwraps its own I/O errors rather than returning them. It is therefore run
on a thread of its own, fed through a bounded channel from the tar writer,
with an output that keeps the first write error instead of passing it up and
raises a flag the tar side sees on its next write. Its decoder reads one frame,
so concatenated frames are read one after another.

### Deliberately not done

- **Hard links are not kept in a new archive.** Each path is stored as its own
  file. Extraction does honour a tar's hard-link entries.
- **Zip entries other than stored and deflate** (deflate64, bzip2, LZMA, zstd,
  encrypted) fail as `archive_unreadable`. Each would be another dependency.
- **Overwrite of an existing folder** is refused rather than merged into.
- **The archive is not re-read to verify it.** "Verified" for a compress means
  the archive exists under its real name after the rename.

### Not verified

- **Interoperability beyond this host.** Archives of all four formats written
  here were listed by GNU tar 1.35, UnZip 6.00, and 7-Zip 23.01, tested by
  `zstd -t` and `unzip -t`, and the `.tar.zst` and `.zip` ones extracted by GNU
  tar and UnZip with their contents and links compared. Archives those tools
  wrote (GNU, ustar, and pax tar, gzip, zstd, and zip) were extracted here with
  contents, links, and modification times compared. That was a manual check
  against this host's tools and is not in the suite, which cannot depend on
  them being installed.
- **A real zip or tar bomb.** The limits are proven with small archives and
  small limits. Nothing tested what happens before the limit is reached on a
  real one, such as a zip whose central directory claims billions of entries:
  the zip crate reads the whole directory before the entry count is checked.
- **Large archives.** Nothing larger than 8 MiB was compressed or extracted,
  and there is no benchmark. The tar.zst path's thread and channel have not
  been measured against a file-sized copy.
- **The window was not opened.** The toolbar buttons and the format chooser
  are drawn from `archive_actions` and `CompressChooser`, and the keys from
  `command_for` and `chooser_key_for`, all of which are tested, but no one has
  clicked the buttons or pressed the keys in a running window. In particular it
  is not confirmed that the window still holds keyboard focus after the
  Compress… button is clicked with a pointer, which is what the chooser's keys
  depend on when it is opened that way rather than with `Ctrl+Shift+P`.
