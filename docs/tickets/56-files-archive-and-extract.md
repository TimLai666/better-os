# Ticket 56 — A person can compress files and extract archives in Better Files

**Epic:** Better Files (Issue #6) · **User Story:** a person can make a `.zip`
or `.tar.gz` of a selection and extract one they downloaded, as a job they can
pause, cancel, and resume · **Branch:** `ticket-56` · **Blocked by:** 51, 52,
53, 54, 55 (they edit the same job engine and window) · **Status:** blocked

## What it delivers

Issue #6 lists archive and extract as job operations, and neither exists.
After this ticket both are `files-operations` job kinds with a menu entry in
the window.

## Acceptance criteria

- [ ] Create `.zip`, `.tar`, `.tar.gz`, and `.tar.zst` from a selection.
- [ ] Extract the same four formats into a new folder named after the archive.
- [ ] Extraction refuses entries whose path escapes the target (absolute paths,
      `..`, symlinks pointing outside) and names the entry.
- [ ] Extraction has limits on total unpacked size and entry count, and stops
      with a clear error when an archive exceeds them.
- [ ] Both are jobs: progress, cancel, and resume behave like copy.
- [ ] New dependencies are pure Rust where one exists, and
      `docs/third-party-licenses.md` is regenerated in the same commit that
      moves `Cargo.lock`.
