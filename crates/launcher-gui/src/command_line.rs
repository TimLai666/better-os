//! What `better-launcher` does with the arguments it was started with.
//!
//! Two invocations open the overlay: none, which is what the keyboard shortcut
//! runs and toggles an open overlay closed, and `--open`, which is what the
//! desktop entry runs. `--help` and `--version` are answered and anything else
//! is refused. `main` asks before it touches the session bus, so neither an
//! answer nor a refusal can reach a launcher that is already on screen.

use better_ui::command_line::{WindowCommandLine, WindowOption};

pub const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-launcher",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[WindowOption::Flag("--open")],
    refusal,
};

fn refusal(other: &str) -> String {
    format!(
        "better-launcher：不接受「{other}」這個參數。可用的參數：--open、--version、--help。\n\
         better-launcher: takes no argument \"{other}\". It accepts --open, --version, and --help."
    )
}

const HELP: &str = "\
better-launcher — Better Launcher

用法 / Usage:
  better-launcher              開啟；已開啟時關閉 / open it, or close it when it is open
  better-launcher --open       開啟，已開啟時維持開啟 / open it, and keep it open when it is
  better-launcher --version    顯示版本 / print the version
  better-launcher --help       顯示這段說明 / print this help";

#[cfg(test)]
mod tests {
    use super::COMMAND_LINE;
    use better_ui::command_line::{Invocation, WindowOption};

    fn run(values: &[&str]) -> Invocation {
        let arguments: Vec<String> = values.iter().map(|value| value.to_string()).collect();
        COMMAND_LINE.invocation(&arguments)
    }

    fn printed(invocation: Invocation) -> String {
        match invocation {
            Invocation::Print(text) => text,
            Invocation::OpenWindow => panic!("expected text, the window would open"),
            Invocation::Refuse(text) => panic!("expected text on stdout, got a refusal: {text}"),
        }
    }

    fn refused(invocation: Invocation) -> String {
        match invocation {
            Invocation::Refuse(text) => text,
            Invocation::OpenWindow => panic!("expected a refusal, the window would open"),
            Invocation::Print(text) => panic!("expected a refusal, got: {text}"),
        }
    }

    fn desktop_arguments(entry: &str) -> Vec<&str> {
        let exec = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .expect("the desktop entry has an Exec line");
        exec.split_whitespace().skip(1).collect()
    }

    fn option_name(option: &WindowOption) -> &'static str {
        match *option {
            WindowOption::Flag(name) | WindowOption::Valued(name) => name,
        }
    }

    /// No argument is what the keyboard shortcut runs; `--open` is what the
    /// desktop entry runs. Refusing either would break the launcher.
    #[test]
    fn the_shortcut_and_the_desktop_entry_open_the_overlay() {
        assert_eq!(run(&[]), Invocation::OpenWindow);
        let entry = include_str!("../../../packaging/launcher/better-launcher.desktop");
        assert_eq!(desktop_arguments(entry), ["--open"]);
        assert_eq!(run(&desktop_arguments(entry)), Invocation::OpenWindow);
    }

    #[test]
    fn version_and_help_are_answered_without_an_overlay() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                printed(run(&[flag])),
                format!("better-launcher {}", env!("CARGO_PKG_VERSION"))
            );
        }
        for flag in ["--help", "-h"] {
            let help = printed(run(&[flag]));
            assert!(help.contains("better-launcher --open"), "{help}");
        }
    }

    #[test]
    fn anything_else_is_refused_and_the_refusal_lists_what_is_accepted() {
        for arguments in [
            &["--toggle"][..],
            &["firefox"],
            &["--open", "--close"],
            &["--open=yes"],
        ] {
            let refusal = refused(run(arguments));
            let offending = arguments.last().expect("one argument");
            assert!(refusal.contains(&format!("「{offending}」")), "{refusal}");
            for option in COMMAND_LINE.options {
                assert!(refusal.contains(option_name(option)), "{refusal}");
            }
            assert!(
                refusal.contains("--help") && refusal.contains("--version"),
                "{refusal}"
            );
        }
    }
}
