# Ticket 70 — A contributor can keep the build tree from filling the disk

**Epic:** Build and release · **User Story:** a contributor runs one command
that frees the build space no longer needed, without losing the build they are
using · **Branch:** `ticket-70` · **Blocked by:** none · **Status:** implemented, not merged

## What it delivers

`target/` reached 158 GB during the v0.2.4 work and 143 GB again before tickets
50–62. The owner decided on 2026-09-27 on a cleanup someone runs:
`packaging/prune-target.sh`.

## Acceptance criteria

- [x] With no options it reports what it would remove and how much, and removes
      nothing. `--apply` removes it.
      *The dry-run and apply sections of `packaging/tests/prune-target.test.sh`.*
- [x] It removes incremental compilation caches, stale artifacts of other
      toolchains' target directories nested in `target/` (for example
      `target/msrv-check`), and release artifacts older than a given age, and
      keeps the current debug build usable.
      *The test's removed and kept lists, and a real cargo project checked by
      hand: after `--apply`, `cargo build` compiled nothing.*
- [x] It accepts a target directory argument so it works on a worktree's own
      `target/`, and refuses a path that is not a cargo target directory
      (no `CACHEDIR.TAG` with cargo's signature).
      *The refusals section of the test.*
- [x] `--help` documents every option; `shellcheck` passes; a test drives it
      against a synthetic target directory.
      *`shellcheck` is not installed on the machine this was built on; it
      passed in the CI `installer` job of run 36300247926 on this branch.*

## What was built

### The script

`packaging/prune-target.sh [--apply] [--older-than DAYS] [TARGET_DIR]`.
With no options it lists what it would remove, grouped, with each path's size
and a total, and removes nothing. `--apply` removes the same list.

It removes three things:

- every `incremental/` directory of a profile (`target/debug`,
  `target/release`, `target/<triple>/<profile>`);
- nested target directories, whole: a directory one or two levels below the
  target that has its own cargo `CACHEDIR.TAG`, such as `target/msrv-check`;
- release artifacts not modified in `DAYS` days (default 7): the entries of
  `release/deps`, `release/build`, `release/.fingerprint` and
  `release/examples`, and the files at the top of `release/`. A `build/` or
  `.fingerprint/` entry with anything newer inside it is kept whole.

Everything else stays: the debug profile's `deps`, `build`, `.fingerprint` and
binaries, cargo's lock files, `doc/`, `tmp/`, and anything under a cache tag
that is not cargo's. A profile directory is one with a `.fingerprint/`, so a
test's scratch directory called `release` or `incremental` is left alone.
Removing `incremental/` does not make cargo rebuild anything; a later edit just
compiles that crate once without its cache. There is no `--all`: `cargo clean`
already does that.

### Guards

`TARGET_DIR` defaults to `target/` in the checkout the script lives in, so a
worktree's copy prunes that worktree's target. The path is resolved through
symlinks first. The script refuses `/`, the home directory, the repository
root, any directory containing one of those, and any directory without a
`CACHEDIR.TAG` whose first line is the cache-tag signature and which says it
was created by cargo. Before removing, it checks that every path lies inside
the resolved target. It only removes with `rm -rf --`, and `find` never
follows a symlink, so a link inside the target is removed as a link.

Sizes come from `du`. Cargo hard-links each top-level binary to a file in
`deps/`, so each listed path shows its full size while the total counts a
shared file once.

Exit status is 0 on success (including nothing to remove), 1 for a refused
target, a missing command or a failed removal, and 2 for bad arguments.

### Test and CI

`packaging/tests/prune-target.test.sh` builds a synthetic target in `mktemp`:
debug and release profiles with old and new files, a hard-linked binary, a
symlink pointing outside the target, a cross-compilation profile, two nested
cargo targets, a directory with a non-cargo cache tag, and scratch
directories named like cargo's. It checks that a dry run leaves every name,
type and size in the work directory unchanged, that `--apply` removes exactly
the expected paths, that `--older-than 0` takes the fresh release artifacts
too, and that every dangerous or untagged path is refused with the right reason
and exit status.

The CI `installer` job gains a step that runs `bash -n`, `shellcheck` and the
test.

### Gates

- `bash packaging/tests/prune-target.test.sh`: 91 checks pass. It failed first
  with the script missing, and again for a hard-linked binary missing from the
  list, cargo's newer lock files being removed, and scratch directories being
  treated as profiles, before each fix.
- `bash -n` on both files: clean.
- `shellcheck`: not installed on this machine; passed in the CI `installer`
  job of run 36300247926.
- Dry run on `ticket-64/target` (11 GB): 1 path, the 2 GB debug
  `incremental/`. The main checkout had no `target/` at the time.
