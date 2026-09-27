# Ticket 67 — The copyleft dependencies are reviewed before the next release

**Epic:** Build and release · **User Story:** the project owner knows what each
copyleft dependency obliges before shipping another release · **Branch:**
`ticket-67` · **Blocked by:** none · **Status:** implemented, not merged

## What it delivers

`AGENTS.md` asks for the license implications of every copyleft dependency to be
reviewed before release, and no review exists. After this ticket
`docs/copyleft-review.md` lists every dependency in the locked graph whose
license is copyleft or weak copyleft (GPL, LGPL, AGPL, MPL, EPL, CDDL, and
similar), which shipped binaries link it, how it is linked, and what that
obliges under the project's GPL-3.0-or-later license, with anything that needs
the owner's decision called out separately.

## Acceptance criteria

- [x] Every copyleft or weak-copyleft package in `Cargo.lock` appears, taken
      from the generated inventory rather than from memory, with its version and
      exact license expression.
- [x] Each entry says which of the eight packages ship it and whether it is
      statically linked into a Rust binary or loaded at run time.
- [x] Each entry states the obligation and whether the current packages meet
      it, citing the file in the package that meets it where one does.
- [x] Anything that is incompatible with GPL-3.0-or-later, or unclear, is listed
      as a decision for the owner rather than resolved in the document.

## What was built

`docs/copyleft-review.md` is the review. It takes every package from
`cargo metadata --format-version 1 --locked --offline` against the lockfile the
inventory pins, finds which shipped binary reaches each one with `cargo tree`
over the same fifteen crates `packaging/build-deb.sh` builds, and checks the
answer against the 32 published v0.2.8 packages, which were unpacked and read
rather than installed.

Five copyleft packages are linked into shipped binaries: `zlog`, `ztracing`,
and `ztracing_macro` (GPL-3.0-or-later), `option-ext` (MPL-2.0), and
`self_cell` (Apache-2.0 OR GPL-2.0-only). All five come in through GPUI, so
only the six windows carry them; `better-storage`, `better-manager-daemon`, and
every command-line, service, and tray binary carry none. `r-efi`, `cbindgen`,
and `dwrote` are in the lockfile but are never built for Linux.

The review also found copyleft code the inventory cannot see. `freetype-sys`
compiles its bundled FreeType 2.13.2 when the build host's FreeType is older
than 2.12.1, and the v0.2.8 packages for Ubuntu 22.04 link it statically.
FreeType is FTL OR GPL-2.0-or-later, and no package mentions it.

Four decisions are left to the owner, each with a recommendation: which
FreeType license the 22.04 packages rely on; recording `self_cell`'s Apache-2.0
branch and shipping the Apache-2.0 text, which no package carries today; whether
to keep relying on crates.io and upstream repositories for the dependency
source or attach a vendored archive to each release; and the missing license
field on `gpui_shared_string` and `gpui_util`.

No code, manifest, or lockfile changed.
