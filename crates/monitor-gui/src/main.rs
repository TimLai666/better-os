mod app;
mod i18n;
mod layout;
mod link;
mod pages;
mod render;
mod shell;
mod stored;
mod tables;
#[cfg(test)]
mod tests;

use app::MonitorApp;
use better_ui::command_line::WindowCommandLine;
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

/// What this window does with the arguments it was started with.
///
/// `/usr/bin/better-monitor` is the window and `better-monitor-cli` is the
/// command line, so `better-monitor inspect` is the command line's usage typed
/// at the window's name. It is refused with the name that would have answered
/// it rather than opening a window a person at a terminal cannot escape.
const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-monitor",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[],
    refusal,
};

fn refusal(other: &str) -> String {
    format!(
        "better-monitor：這是 Better Monitor 的視窗，不接受「{other}」這個參數。\n\
         指令列請改用 better-monitor-cli，例如 better-monitor-cli inspect。\n\
         better-monitor: this is the Better Monitor window and takes no argument \"{other}\".\n\
         For the command line use better-monitor-cli, e.g. better-monitor-cli inspect."
    )
}

const HELP: &str = "\
better-monitor — Better Monitor 的視窗 / the Better Monitor window

用法 / Usage:
  better-monitor              開啟視窗 / open the window
  better-monitor --version    顯示版本 / print the version
  better-monitor --help       顯示這段說明 / print this help

指令列是另一個程式 / The command line is a separate program:
  better-monitor-cli --help";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    COMMAND_LINE.open_or_exit(&arguments);

    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // `gpui_component::init` installs its light theme. Better OS is
        // dark-first, so the window applies the dark theme once it exists.
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1320.0), px(860.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options("io.betteros.Monitor")
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| MonitorApp::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open Better Monitor window");
        })
        .detach();
    });
}

#[cfg(test)]
mod invocation_tests {
    use super::COMMAND_LINE;
    use better_ui::command_line::Invocation;

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

    #[test]
    fn no_argument_and_the_desktop_entry_open_the_window() {
        assert_eq!(run(&[]), Invocation::OpenWindow);
        let entry = include_str!("../../../packaging/monitor/io.betteros.Monitor.desktop");
        assert_eq!(run(&desktop_arguments(entry)), Invocation::OpenWindow);
    }

    #[test]
    fn version_and_help_are_answered_without_a_window() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                printed(run(&[flag])),
                format!("better-monitor {}", env!("CARGO_PKG_VERSION"))
            );
        }
        for flag in ["--help", "-h"] {
            let help = printed(run(&[flag]));
            assert!(help.contains("better-monitor-cli --help"), "{help}");
        }
    }

    /// `better-monitor inspect` is the command line's usage typed at the
    /// window's name, which is the mistake this refusal exists for.
    #[test]
    fn a_command_line_argument_is_refused_and_names_the_command_line() {
        for arguments in [&["inspect"][..], &["--offline", "inspect"], &["--bogus"]] {
            let refusal = refused(run(arguments));
            assert!(
                refusal.contains(&format!("「{}」", arguments[0])),
                "{refusal}"
            );
            assert!(refusal.contains("better-monitor-cli inspect"), "{refusal}");
        }
    }
}
