# Ticket 67 — The copyleft dependencies are reviewed before the next release

**Epic:** Build and release · **User Story:** the project owner knows what each
copyleft dependency obliges before shipping another release · **Branch:**
`ticket-67` · **Blocked by:** none · **Status:** ready

## What it delivers

`AGENTS.md` asks for the license implications of every copyleft dependency to be
reviewed before release, and no review exists. After this ticket
`docs/copyleft-review.md` lists every dependency in the locked graph whose
license is copyleft or weak copyleft (GPL, LGPL, AGPL, MPL, EPL, CDDL, and
similar), which shipped binaries link it, how it is linked, and what that
obliges under the project's GPL-3.0-or-later license, with anything that needs
the owner's decision called out separately.

## Acceptance criteria

- [ ] Every copyleft or weak-copyleft package in `Cargo.lock` appears, taken
      from the generated inventory rather than from memory, with its version and
      exact license expression.
- [ ] Each entry says which of the eight packages ship it and whether it is
      statically linked into a Rust binary or loaded at run time.
- [ ] Each entry states the obligation and whether the current packages meet
      it, citing the file in the package that meets it where one does.
- [ ] Anything that is incompatible with GPL-3.0-or-later, or unclear, is listed
      as a decision for the owner rather than resolved in the document.
