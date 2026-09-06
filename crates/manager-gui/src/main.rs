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
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

/// What to do with the arguments this window was started with.
///
/// `/usr/bin/better-manager` is the window, and the command line ships beside
/// it as `better-manager-cli` — the same split `better-monitor` already has.
/// Until ticket 49 this binary ignored every argument and opened a window
/// anyway, so `better-manager catalog status` looked like a command that hung:
/// it had run the window, which does not exit.
///
/// Only `--help` and `--version` are answered here, because those are the two
/// a person types at a window's name expecting an answer rather than a window.
enum Invocation {
    OpenWindow,
    Print(String),
    Refuse(String),
}

fn invocation(arguments: &[String]) -> Invocation {
    let Some(first) = arguments.first() else {
        return Invocation::OpenWindow;
    };
    match first.as_str() {
        "--version" | "-V" => {
            Invocation::Print(format!("better-manager {}", env!("CARGO_PKG_VERSION")))
        }
        "--help" | "-h" => Invocation::Print(HELP.to_string()),
        other => Invocation::Refuse(format!(
            "better-manager：這是元件管理器的視窗，不接受「{other}」這個參數。\n\
             指令列請改用 better-manager-cli，例如 better-manager-cli catalog status。\n\
             better-manager: this is the manager window and takes no argument \"{other}\".\n\
             For the command line use better-manager-cli, e.g. better-manager-cli catalog status."
        )),
    }
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
    match invocation(&arguments) {
        Invocation::OpenWindow => {}
        Invocation::Print(text) => {
            println!("{text}");
            return;
        }
        Invocation::Refuse(text) => {
            eprintln!("{text}");
            std::process::exit(2);
        }
    }

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
