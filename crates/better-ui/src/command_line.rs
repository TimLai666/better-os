//! What a window does with the arguments it was started with.
//!
//! A window that ignores its arguments turns a question typed at a terminal
//! into a window the person waiting there cannot escape: before ticket 49,
//! `better-manager catalog status` opened the manager's window and never
//! returned. Every Better OS window therefore answers `--help` and `--version`
//! itself, accepts the flags it declares, and refuses anything else with exit
//! status 2, before any window, bus connection, or setting is touched.
//!
//! This module has no GPUI in it. Each window describes its own command line
//! with a [`WindowCommandLine`] and owns the wording of its help and refusal;
//! the scanning is here, once.

/// An argument a window understands, besides `--help` and `--version`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowOption {
    /// A flag on its own, such as `--open`.
    Flag(&'static str),
    /// A flag followed by a value, as `--lang en-US` or `--lang=en-US`. A
    /// missing value is left for the window to default, as it always was.
    Valued(&'static str),
}

/// One window's command line.
#[derive(Clone, Copy, Debug)]
pub struct WindowCommandLine {
    /// The name the window is installed under, as `--version` prints it.
    pub program: &'static str,
    /// The window's own version, normally `env!("CARGO_PKG_VERSION")`.
    pub version: &'static str,
    /// What `--help` prints.
    pub help: &'static str,
    /// Everything the window accepts besides `--help` and `--version`.
    pub options: &'static [WindowOption],
    /// The message for an argument the window does not understand. It is given
    /// that argument and should say where the person should go instead.
    pub refusal: fn(&str) -> String,
}

/// What to do with the arguments a window was started with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    /// Every argument is one the window accepts.
    OpenWindow,
    /// Print this to stdout and exit successfully.
    Print(String),
    /// Print this to stderr and exit with status 2.
    Refuse(String),
}

impl WindowCommandLine {
    /// Reads the arguments left to right, without the program name. The first
    /// `--help` or `--version` wins, and so does the first argument the window
    /// does not accept.
    pub fn invocation(&self, arguments: &[String]) -> Invocation {
        let mut arguments = arguments.iter();
        'arguments: while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--help" | "-h" => return Invocation::Print(self.help.to_string()),
                "--version" | "-V" => {
                    return Invocation::Print(format!("{} {}", self.program, self.version));
                }
                _ => {}
            }
            for option in self.options {
                match *option {
                    WindowOption::Flag(name) if argument == name => continue 'arguments,
                    WindowOption::Valued(name) if argument == name => {
                        arguments.next();
                        continue 'arguments;
                    }
                    WindowOption::Valued(name)
                        if argument
                            .strip_prefix(name)
                            .is_some_and(|rest| rest.starts_with('=')) =>
                    {
                        continue 'arguments;
                    }
                    _ => {}
                }
            }
            return Invocation::Refuse((self.refusal)(argument));
        }
        Invocation::OpenWindow
    }

    /// Answers `--help` and `--version` and refuses what the window does not
    /// accept, ending the process in each of those cases. It returns only when
    /// the window should open.
    pub fn open_or_exit(&self, arguments: &[String]) {
        match self.invocation(arguments) {
            Invocation::OpenWindow => {}
            Invocation::Print(text) => {
                println!("{text}");
                std::process::exit(0);
            }
            Invocation::Refuse(text) => {
                eprintln!("{text}");
                std::process::exit(2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: WindowCommandLine = WindowCommandLine {
        program: "sample-window",
        version: "1.2.3",
        help: "sample help",
        options: &[WindowOption::Flag("--open"), WindowOption::Valued("--lang")],
        refusal: |argument| format!("refused {argument}"),
    };

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn run(values: &[&str]) -> Invocation {
        SAMPLE.invocation(&arguments(values))
    }

    #[test]
    fn no_argument_opens_the_window() {
        assert_eq!(run(&[]), Invocation::OpenWindow);
    }

    #[test]
    fn help_and_version_are_answered_in_both_spellings() {
        for flag in ["--help", "-h"] {
            assert_eq!(run(&[flag]), Invocation::Print("sample help".into()));
        }
        for flag in ["--version", "-V"] {
            assert_eq!(
                run(&[flag]),
                Invocation::Print("sample-window 1.2.3".into())
            );
        }
    }

    #[test]
    fn declared_flags_open_the_window() {
        assert_eq!(run(&["--open"]), Invocation::OpenWindow);
        assert_eq!(run(&["--lang", "en-US"]), Invocation::OpenWindow);
        assert_eq!(run(&["--lang=zh-TW", "--open"]), Invocation::OpenWindow);
        // A valued flag at the end keeps meaning "use the default".
        assert_eq!(run(&["--open", "--lang"]), Invocation::OpenWindow);
    }

    #[test]
    fn a_valued_flag_takes_the_next_argument_whatever_it_looks_like() {
        assert_eq!(run(&["--lang", "catalog"]), Invocation::OpenWindow);
    }

    #[test]
    fn anything_else_is_refused_with_the_windows_own_message() {
        for argument in ["catalog", "--bogus", "-x", "", "--open=yes", "--language"] {
            assert_eq!(
                run(&[argument]),
                Invocation::Refuse(format!("refused {argument}")),
                "{argument:?}"
            );
        }
        assert_eq!(
            run(&["--open", "status"]),
            Invocation::Refuse("refused status".into())
        );
    }

    #[test]
    fn the_first_answer_wins_reading_left_to_right() {
        assert_eq!(
            run(&["--help", "--bogus"]),
            Invocation::Print("sample help".into())
        );
        assert_eq!(
            run(&["--bogus", "--help"]),
            Invocation::Refuse("refused --bogus".into())
        );
        assert_eq!(
            run(&["--open", "--version", "--help"]),
            Invocation::Print("sample-window 1.2.3".into())
        );
    }
}
