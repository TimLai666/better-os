//! The Better Awake application window.
//!
//! Eight sections, one sidebar, and one contract: this process never holds an
//! inhibitor, never writes a power setting, and never runs a shell command.
//! Everything it shows came from `awake-service` over `awake-ipc`, and every
//! change it makes goes back the same way. Closing the window ends nothing.

mod app;
mod client;
mod components;
mod i18n;
mod layout;
mod localtime;
mod model;
mod pages_records;
mod pages_rules;
mod pages_status;
mod render;
mod settings;
mod shell;
#[cfg(test)]
mod tests;

use app::AwakeApp;
use better_ui::command_line::WindowCommandLine;
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

/// What this window does with the arguments it was started with.
///
/// The desktop entry and the tray both start it with none. Better Awake has no
/// command line of its own, so the window answers `--help` and `--version` and
/// refuses everything else rather than opening.
const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "awake-gui",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[],
    refusal,
};

fn refusal(other: &str) -> String {
    format!(
        "awake-gui：這是 Better Awake 的視窗，不接受「{other}」這個參數。只接受 --help 與 --version。\n\
         awake-gui: this is the Better Awake window and takes no argument \"{other}\". \
         It accepts only --help and --version."
    )
}

const HELP: &str = "\
awake-gui — Better Awake 的視窗 / the Better Awake window

用法 / Usage:
  awake-gui              開啟視窗 / open the window
  awake-gui --version    顯示版本 / print the version
  awake-gui --help       顯示這段說明 / print this help";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    COMMAND_LINE.open_or_exit(&arguments);

    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // gpui-component installs its light theme here. Better OS is
        // dark-first, so the stored appearance is applied once the window
        // exists and the saved preferences have been read.
        gpui_component::init(cx);
        cx.bind_keys(shell::key_bindings());

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.0), px(820.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options("better-awake")
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| AwakeApp::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open the Better Awake window");
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

    /// The desktop entry and the tray both start the window with no argument.
    #[test]
    fn no_argument_and_the_desktop_entry_open_the_window() {
        assert_eq!(run(&[]), Invocation::OpenWindow);
        let entry = include_str!("../../../packaging/awake/better-awake.desktop");
        assert_eq!(run(&desktop_arguments(entry)), Invocation::OpenWindow);
    }

    #[test]
    fn version_and_help_are_answered_without_a_window() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                printed(run(&[flag])),
                format!("awake-gui {}", env!("CARGO_PKG_VERSION"))
            );
        }
        for flag in ["--help", "-h"] {
            let help = printed(run(&[flag]));
            assert!(help.contains("awake-gui --version"), "{help}");
        }
    }

    #[test]
    fn every_other_argument_is_refused() {
        for argument in ["--open", "status", "--offline", "-x"] {
            let refusal = refused(run(&[argument]));
            assert!(refusal.contains(&format!("「{argument}」")), "{refusal}");
            assert!(
                refusal.contains("--help") && refusal.contains("--version"),
                "{refusal}"
            );
        }
    }
}
