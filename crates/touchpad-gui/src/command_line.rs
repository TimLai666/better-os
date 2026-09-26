//! What `better-touchpad` does with the arguments it was started with.
//!
//! These are the flags the window already read — both desktop entries use
//! them — plus `--help` and `--version`. Anything else is refused before safe
//! mode is written, a setting is read, or a window opens.

use better_ui::command_line::{WindowCommandLine, WindowOption};

pub const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-touchpad",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[
        WindowOption::Valued("--lang"),
        WindowOption::Valued("--page"),
        WindowOption::Flag("--offline"),
        WindowOption::Flag("--safe-mode"),
        WindowOption::Flag("--normal-mode"),
    ],
    refusal,
};

fn refusal(other: &str) -> String {
    format!(
        "better-touchpad：不接受「{other}」這個參數。可用的參數：--lang、--page、--offline、\
         --safe-mode、--normal-mode、--version、--help。\n\
         better-touchpad: takes no argument \"{other}\". It accepts --lang, --page, --offline, \
         --safe-mode, --normal-mode, --version, and --help."
    )
}

const HELP: &str = "\
better-touchpad — Better Touchpad

用法 / Usage:
  better-touchpad                       開啟視窗 / open the window
  better-touchpad --lang zh-TW|en-US    以指定語言開啟 / open in that language
  better-touchpad --page <page>         開啟到指定畫面 / open on that screen:
                                        overview, pointer, scrolling, clicking,
                                        gestures, devices, diagnostics
  better-touchpad --offline             不連線到工作階段匯流排 / do not use the session bus
  better-touchpad --safe-mode           關閉 Better Touchpad 整合後結束 / turn the integration off and exit
  better-touchpad --normal-mode         重新開啟整合後結束 / turn it back on and exit
  better-touchpad --version             顯示版本 / print the version
  better-touchpad --help                顯示這段說明 / print this help";

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

    #[test]
    fn both_desktop_entries_are_accepted() {
        assert_eq!(run(&[]), Invocation::OpenWindow);
        for entry in [
            include_str!("../../../packaging/touchpad/better-touchpad.desktop"),
            include_str!("../../../packaging/touchpad/better-touchpad-safe-mode.desktop"),
        ] {
            assert_eq!(run(&desktop_arguments(entry)), Invocation::OpenWindow);
        }
    }

    /// Every flag the window already read keeps working, in both spellings a
    /// valued flag had.
    #[test]
    fn the_existing_flags_are_accepted() {
        for arguments in [
            &["--safe-mode"][..],
            &["--normal-mode"],
            &["--offline"],
            &["--lang", "en-US"],
            &["--lang=zh-TW"],
            &["--page", "gestures"],
            &["--page=devices"],
            &["--offline", "--page", "gestures", "--lang", "zh-TW"],
            // A valued flag with nothing after it still means the default.
            &["--lang"],
        ] {
            assert_eq!(run(arguments), Invocation::OpenWindow, "{arguments:?}");
        }
    }

    #[test]
    fn version_and_help_are_answered_without_a_window() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                printed(run(&[flag])),
                format!("better-touchpad {}", env!("CARGO_PKG_VERSION"))
            );
        }
        for flag in ["--help", "-h"] {
            let help = printed(run(&[flag]));
            for option in COMMAND_LINE.options {
                assert!(help.contains(option_name(option)), "{help}");
            }
        }
    }

    /// The help names every screen `--page` can open, from the one list the
    /// window itself parses.
    #[test]
    fn help_lists_every_page() {
        let help = printed(run(&["--help"]));
        for page in crate::Page::ALL {
            assert!(
                help.contains(page.key()),
                "{} missing from:\n{help}",
                page.key()
            );
        }
    }

    #[test]
    fn anything_else_is_refused_and_the_refusal_lists_what_is_accepted() {
        for arguments in [
            &["--bogus"][..],
            &["gestures"],
            &["--offline", "--safe"],
            &["--offline=yes"],
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
