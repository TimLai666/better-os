mod app;
mod components;
mod i18n;
mod model;
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

fn main() {
    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // gpui-component initializes its light theme here. Better Manager's
        // first release deliberately uses a light system-utility appearance.
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(820.0)), cx)),
            ..Default::default()
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
