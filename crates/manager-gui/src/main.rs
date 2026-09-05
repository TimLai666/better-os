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

fn main() {
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
