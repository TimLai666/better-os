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
use app_chooser_gui::{
    AppChooser, ChooserEvent, ChooserMode, Locale, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};
use gpui::*;
use gpui_component::{ActiveTheme, Icon, IconName, Root, Sizable as _, Theme, ThemeMode, v_flex};
use gpui_component_assets::Assets;

/// The standalone window around the reusable chooser.
///
/// The titlebar lives here rather than in `AppChooser` itself, because the same
/// surface is also drawn inside Better Files as an overlay — and an overlay
/// with a window titlebar in the middle of another window would be wrong.
struct StandaloneChooser {
    chooser: Entity<AppChooser>,
    /// The window title says which of the two jobs this chooser was opened
    /// for, in the same words the surface's own heading uses.
    title: &'static str,
}

impl Render for StandaloneChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Mutter gives an `xdg-toplevel` client no decorations, so this window
        // draws its own or it cannot be closed, minimized, maximized or moved.
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(better_ui::window_chrome::title_bar(
                Icon::new(IconName::SquareTerminal).small(),
                self.title,
                cx.theme().foreground,
            ))
            .child(div().flex_1().min_h_0().child(self.chooser.clone()))
    }
}

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
            ..better_ui::window_chrome::window_options("io.betteros.AppChooser")
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
                let c = app_chooser_gui::i18n::copy(Locale::System);
                let title = match mode {
                    ChooserMode::OpenWith => c.open_with_title,
                    ChooserMode::ChooseExecutable => c.executable_title,
                };
                let root = cx.new(|_| StandaloneChooser { chooser, title });
                cx.new(|cx| Root::new(root, window, cx))
            })
            .expect("failed to open the Better App Chooser window");
        })
        .detach();
    });
}
