# Ticket 60 — A window given an argument it does not understand says so

**Epic:** Cross-component polish · **User Story:** a person who types
`better-monitor --help` in a terminal is told where the command line is instead
of getting a window · **Branch:** `ticket-60-62` · **Blocked by:** none ·
**Status:** implemented, not merged

## What it delivers

Ticket 49 made `better-manager` refuse an argument it does not understand with
exit 2 and name `better-manager-cli`. Better Monitor, Better Launcher, Better
Touchpad, and Better Awake's window still swallow their arguments. Separately,
`better-monitor-cli --help` and `better-manager-cli --help` print the names
`better-monitor` and `better-manager`, which are the windows.

## Acceptance criteria

- [x] `better-monitor` refuses unknown arguments and names `better-monitor-cli`.
- [x] `better-launcher` and `better-touchpad` keep their existing flags and
      refuse anything else, listing the flags they accept.
- [x] `awake-gui` refuses any argument other than `--help` and `--version`.
- [x] Each window answers `--help` and `--version` without opening a window.
- [x] The two command lines print their installed names in `--help` and in
      their own error messages.
- [x] Each window's argument handling is a pure function with unit tests, and
      `manager-gui`'s existing `invocation()` gains the tests it lacks.

## What was built

`better_ui::command_line` is the one place a window reads its arguments. A
window describes its command line as a `WindowCommandLine`: its installed name,
its version, its help text, the flags it accepts, and a function that words the
refusal. `invocation()` reads the arguments left to right, answers `--help`/`-h`
and `--version`/`-V`, accepts the declared flags (a valued flag as `--lang x` or
`--lang=x`), and refuses the first argument it does not recognize.
`open_or_exit()` prints and exits 0 for an answer, prints to stderr and exits 2
for a refusal, and returns only when the window should open. The module has no
GPUI in it, and it lives in `better-ui` because every window already depends on
that crate, so no dependency and no lockfile line moved.

Each window now calls it as the first thing `main` does:

- `better-manager` moved onto it. Its wording, its help, and its behaviour are
  unchanged, and its `invocation()` gained the unit tests it lacked.
- `better-monitor` accepts nothing but `--help` and `--version`, and a refusal
  names `better-monitor-cli`, with `better-monitor-cli inspect` as the example.
- `better-launcher` accepts `--open`, and a refusal lists `--open`, `--version`,
  and `--help`. The check runs before the session bus is touched, so `--help` or
  a typo cannot toggle a launcher that is already on screen.
- `better-touchpad` accepts `--lang`, `--page`, `--offline`, `--safe-mode`, and
  `--normal-mode`, and its help and refusal list all of them. A valued flag with
  nothing after it still means "use the default", as it did before.
- `awake-gui` accepts only `--help` and `--version`.

Both desktop entries' `Exec` lines are asserted to open the window, per window,
by reading the shipped `.desktop` file rather than a copy of it. Each window
also has an integration test that runs the built binary with `--help`,
`--version`, and an unknown argument under a timeout, so "answers without
opening a window" is observed rather than inferred.

The two command lines now call themselves by their installed names.
`monitor-cli` and `manager-cli` set clap's `name` and `bin_name` to
`better-monitor-cli` and `better-manager-cli`, so `--help`, `--version`, and
clap's usage errors say those names whatever the process was started as. The
cargo binaries keep their names (`better-monitor` and `manager-cli`), because
`build-deb.sh` maps them to the installed paths and renaming them would change
packaging for no user-visible gain. `better-monitor-cli` also prints its own
name in front of a runtime error and in the "nothing is recording" advice. The
comments in `build-deb.sh` and `better-monitor.yaml` that described the old
mismatch now describe the fix.

Deliberately not done: `better-files` is left to its own ticket. The refusal
wording stays bilingual in one message, like the manager's, rather than
following the desktop locale, because it is printed before any locale is read.

Not verified: the refusals were run from the debug binaries on this host and in
the tests; no installed package was exercised.
