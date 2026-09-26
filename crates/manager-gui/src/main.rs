mod app;
mod components;
mod defaults_app;
mod defaults_model;
#[cfg(test)]
mod defaults_tests;
mod i18n;
mod layout;
mod model;
mod pages_defaults;
mod pages_flow;
mod pages_main;
mod pages_settings;
mod render;
mod shell;
#[cfg(test)]
mod tests;

use app::ManagerApp;
use better_ui::command_line::WindowCommandLine;
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

/// What this window does with the arguments it was started with.
///
/// `/usr/bin/better-manager` is the window, and the command line ships beside
/// it as `better-manager-cli` — the same split `better-monitor` already has.
/// Until ticket 49 this binary ignored every argument and opened a window
/// anyway, so `better-manager catalog status` looked like a command that hung:
/// it had run the window, which does not exit.
///
/// Only `--help` and `--version` are answered here, because those are the two
/// a person types at a window's name expecting an answer rather than a window.
const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-manager",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[],
    refusal,
};

fn refusal(other: &str) -> String {
    format!(
        "better-manager：這是元件管理器的視窗，不接受「{other}」這個參數。\n\
         指令列請改用 better-manager-cli，例如 better-manager-cli catalog status。\n\
         better-manager: this is the manager window and takes no argument \"{other}\".\n\
         For the command line use better-manager-cli, e.g. better-manager-cli catalog status."
    )
}

const HELP: &str = "\
better-manager — Better OS 元件管理器的視窗 / the Better OS manager window

用法 / Usage:
  better-manager              開啟視窗 / open the window
  better-manager --version    顯示版本 / print the version
  better-manager --help       顯示這段說明 / print this help

指令列是另一個程式 / The command line is a separate program:
  better-manager-cli --help";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    COMMAND_LINE.open_or_exit(&arguments);

    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // gpui-component installs its light theme here. Better OS is
        // dark-first, so the stored appearance is applied once the window
        // exists and the saved settings have been read.
        gpui_component::init(cx);

        // `io.betteros.Manager` is the desktop entry's file name without its
        // suffix. The compositor matches a window to its entry by exactly that
        // string, so the two have to be changed together.
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(820.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options("io.betteros.Manager")
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| ManagerApp::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open Better OS Manager window");
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

    #[test]
    fn no_argument_opens_the_window() {
        assert!(matches!(run(&[]), Invocation::OpenWindow));
    }

    /// The desktop entry is how almost everyone starts the window, so its own
    /// command line must never be the thing that is refused.
    #[test]
    fn the_desktop_entry_command_opens_the_window() {
        let entry = include_str!("../../../packaging/manager/io.betteros.Manager.desktop");
        let exec = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .expect("the desktop entry has an Exec line");
        let arguments: Vec<&str> = exec.split_whitespace().skip(1).collect();
        assert!(matches!(run(&arguments), Invocation::OpenWindow), "{exec}");
    }

    #[test]
    fn version_names_the_window_and_the_workspace_version() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                printed(run(&[flag])),
                format!("better-manager {}", env!("CARGO_PKG_VERSION"))
            );
        }
    }

    #[test]
    fn help_points_at_the_command_line() {
        for flag in ["--help", "-h"] {
            let help = printed(run(&[flag]));
            assert!(help.contains("better-manager-cli --help"), "{help}");
        }
    }

    #[test]
    fn a_command_line_argument_is_refused_and_redirected() {
        for arguments in [
            &["catalog", "status"][..],
            &["--bogus"],
            &["list"],
            &["reconcile", "--adopt", "better-awake"],
        ] {
            let refusal = refused(run(arguments));
            assert!(
                refusal.contains(&format!("「{}」", arguments[0])),
                "{refusal}"
            );
            assert!(
                refusal.contains("better-manager-cli catalog status"),
                "{refusal}"
            );
        }
    }

    #[test]
    fn the_first_argument_decides() {
        assert!(matches!(run(&["--help", "catalog"]), Invocation::Print(_)));
        assert!(matches!(run(&["catalog", "--help"]), Invocation::Refuse(_)));
    }
}
