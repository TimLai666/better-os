# Ticket 323 — A plugin that cannot be started says why, not "not found"

**Spec:** engineering/specs/files-gui-policy.md (plugins)
**Blocked by:** none

## Why

`crates/files-gui/src/plugins.rs` turns every error from starting a plugin's
program into `RunError::NotFound` (lines 511 and 596:
`.map_err(|_| RunError::NotFound(..))`). A program that exists and cannot be
run (no execute permission, a missing interpreter, a busy file, too many
processes) is reported to the person as not found, which is false and sends
them looking for a file that is there. AGENTS.md: a fallback that turns
"unknown" into a plausible value is a defect.

## What to build

- Keep the error's kind: not found, not permitted, not a program this machine
  can run, and anything else with its system message, each with its sentence
  in zh-TW and English where the plugin's failure is shown.
- Tests for each kind with fixture programs reached as AGENTS.md's test rules
  allow.

## Done when

- [ ] A plugin whose program lacks execute permission says so; one whose file
      is missing says not found; an unrecognised error shows the system's own
      words.
