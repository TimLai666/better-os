# Ticket 146 — One place turns a timestamp into local time

**Blocked by:** none

## Why

Ticket 140's worker found three copies of the local-time conversion
(`awake-gui`, `awake-tray`, `manager-gui`), against AGENTS.md's rule that a
rule two places need lives in one place, and `better-manager-cli` can only
print UTC because the CLI has none of them.

## What to build

- One local-time function in a shared crate (`better-core` or `better-ui`'s
  GPUI-free part), tested across DST changes and with no timezone data
  (reports UTC and says so, never a guessed offset).
- Every copy replaced; the CLI prints local time like the windows.
- Search the workspace for further copies (strftime-like formatting, offset
  arithmetic) and fold them in.

## Done when

- One implementation, its tests, and no other conversion left (a test greps
  for the old helpers).
