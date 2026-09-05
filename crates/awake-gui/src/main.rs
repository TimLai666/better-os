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
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

fn main() {
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
