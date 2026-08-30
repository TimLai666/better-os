//! A standalone window around the reusable chooser.
//!
//! This is how the surface is tested before Better Files exists. It takes a
//! file path and an optional mode, and it exits when the chooser reports a
//! selection or a cancellation.
//!
//! Usage:
//!
//! ```text
//! app-chooser-gui [PATH] [--executable] [--mime TYPE]
//! ```
//!
//! With no path it opens against a placeholder target so the surface can be
//! launched in a headless smoke test with nothing else installed.

use app_chooser_gui::cli::{parse_arguments, target_from};
use app_chooser_gui::{AppChooser, ChooserEvent, Locale, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use gpui::*;
use gpui_component::{Root, Theme, ThemeMode};
use gpui_component_assets::Assets;

fn main() {
    let arguments = parse_arguments(std::env::args().skip(1));
    let target = target_from(&arguments);
    let mode = arguments.mode;
    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(720.0), px(760.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                // Better OS is dark-first; gpui-component installs its light
                // theme at init.
                Theme::change(ThemeMode::Dark, Some(window), cx);
                let chooser =
                    cx.new(|cx| AppChooser::new(target, mode, Locale::System, window, cx));
                cx.subscribe(&chooser, |_, event: &ChooserEvent, cx| {
                    // A standalone chooser has done its job once it has an
                    // answer. An embedded one keeps living; that is the host's
                    // decision, not the surface's.
                    match event {
                        ChooserEvent::Selected(_) | ChooserEvent::Cancelled => cx.quit(),
                    }
                })
                .detach();
                cx.new(|cx| Root::new(chooser, window, cx))
            })
            .expect("failed to open the Better App Chooser window");
        })
        .detach();
    });
}
