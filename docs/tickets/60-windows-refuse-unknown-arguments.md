# Ticket 60 — A window given an argument it does not understand says so

**Epic:** Cross-component polish · **User Story:** a person who types
`better-monitor --help` in a terminal is told where the command line is instead
of getting a window · **Branch:** `ticket-60-62` · **Blocked by:** none ·
**Status:** ready

## What it delivers

Ticket 49 made `better-manager` refuse an argument it does not understand with
exit 2 and name `better-manager-cli`. Better Monitor, Better Launcher, Better
Touchpad, and Better Awake's window still swallow their arguments. Separately,
`better-monitor-cli --help` and `better-manager-cli --help` print the names
`better-monitor` and `better-manager`, which are the windows.

## Acceptance criteria

- [ ] `better-monitor` refuses unknown arguments and names `better-monitor-cli`.
- [ ] `better-launcher` and `better-touchpad` keep their existing flags and
      refuse anything else, listing the flags they accept.
- [ ] `awake-gui` refuses any argument other than `--help` and `--version`.
- [ ] Each window answers `--help` and `--version` without opening a window.
- [ ] The two command lines print their installed names in `--help` and in
      their own error messages.
- [ ] Each window's argument handling is a pure function with unit tests, and
      `manager-gui`'s existing `invocation()` gains the tests it lacks.
