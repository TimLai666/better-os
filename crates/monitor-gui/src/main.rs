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
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

fn main() {
    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // `gpui_component::init` installs its light theme. Better OS is
        // dark-first, so the window applies the dark theme once it exists.
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1320.0), px(860.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..Default::default()
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
