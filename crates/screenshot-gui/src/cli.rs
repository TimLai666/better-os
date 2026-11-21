//! Better Screenshot's command line, through `better_ui::command_line` like
//! every Better OS window (AGENTS.md): it answers `--help` and `--version`,
//! takes the flags the key bindings and the portal's activation pass, and
//! refuses anything else with exit status 2.

use better_ui::command_line::{WindowCommandLine, WindowOption};

pub const PROGRAM: &str = "better-screenshot";

pub const HELP: &str = "\
better-screenshot: take a picture of the screen or record it (Better OS session)

Usage:
  better-screenshot            open the interface (Print Screen); stops a recording
  better-screenshot --screen   take every screen at once (Shift+Print)
  better-screenshot --window   take the focused window at once (Alt+Print)
  better-screenshot --record   open the interface to record (Ctrl+Shift+Alt+R); stops a recording
  better-screenshot --portal   serve the Screenshot portal (started by D-Bus)
  better-screenshot --help     this text
  better-screenshot --version  the version

Pictures go to the clipboard and to Pictures/Screenshots; recordings to
Videos/Screencasts. One process at a time: a second start hands its request
to the first.
";

fn refusal(argument: &str) -> String {
    format!("better-screenshot: unknown argument {argument:?}; see better-screenshot --help")
}

pub const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: PROGRAM,
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[
        WindowOption::Flag("--screen"),
        WindowOption::Flag("--window"),
        WindowOption::Flag("--record"),
        WindowOption::Flag("--portal"),
    ],
    refusal,
};

#[cfg(test)]
mod tests {
    use super::COMMAND_LINE;
    use better_ui::command_line::Invocation;
    use screenshot_core::session::Request;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_key_bindings_flag_is_accepted_and_means_its_request() {
        for (flag, request) in [
            ("--screen", Request::Screen),
            ("--window", Request::Window),
            ("--record", Request::Recording),
            ("--portal", Request::Portal),
        ] {
            assert_eq!(COMMAND_LINE.invocation(&args(&[flag])), Invocation::OpenWindow);
            assert_eq!(Request::from_arguments(&args(&[flag])), request);
        }
        assert_eq!(COMMAND_LINE.invocation(&args(&[])), Invocation::OpenWindow);
    }

    #[test]
    fn help_and_version_print_and_anything_else_is_refused() {
        match COMMAND_LINE.invocation(&args(&["--help"])) {
            Invocation::Print(text) => assert!(text.contains("--screen")),
            other => panic!("{other:?}"),
        }
        match COMMAND_LINE.invocation(&args(&["--version"])) {
            Invocation::Print(text) => assert!(text.contains(env!("CARGO_PKG_VERSION"))),
            other => panic!("{other:?}"),
        }
        match COMMAND_LINE.invocation(&args(&["--delay", "5"])) {
            Invocation::Refuse(text) => assert!(text.contains("--delay")),
            other => panic!("{other:?}"),
        }
    }
}
