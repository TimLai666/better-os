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
//!
//! The window takes the appearance stored for it
//! (`$XDG_CONFIG_HOME/better-os/app-chooser/appearance`: system, light, or
//! dark), and follows the desktop when none is stored, as ADR 0019 sets for
//! every window.

use app_chooser_gui::cli::{parse_arguments, target_from};
use app_chooser_gui::{
    AppChooser, ChooserEvent, ChooserMode, DIALOG_WIDTH, Locale, MIN_WINDOW_HEIGHT,
    MIN_WINDOW_WIDTH, dialog_height,
};
use better_ui::fluent::chrome::TitleBar;
use better_ui::fluent::palette;
use better_ui::fluent::theme::Appearance;
use gpui::*;
use gpui_component::{Icon, IconName, Root};
use gpui_component_assets::Assets;

/// The standalone window around the reusable chooser: a dialog window,
/// sized to what the dialog holds.
///
/// The titlebar lives here rather than in `AppChooser` itself, because the same
/// surface is also drawn inside Better Files — and a window titlebar in the
/// middle of another window would be wrong. It is a dialog's bar: the close
/// button alone.
struct StandaloneChooser {
    chooser: Entity<AppChooser>,
    title: &'static str,
    _observe: Subscription,
}

impl StandaloneChooser {
    fn new(
        chooser: Entity<AppChooser>,
        title: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let appearance = Appearance::default_path("app-chooser")
            .map(|path| Appearance::load(&path))
            .unwrap_or_default();
        appearance.apply_and_follow(window, cx);
        let observe = cx.observe_in(&chooser, window, |_, chooser, window, cx| {
            let chooser = chooser.read(cx);
            // Asked again on every change, not only when the height moved:
            // a compositor may ignore a request (a tiled window) and honour
            // the same one later (once it floats).
            // The program-picking mode holds a browser under the list and
            // takes the dialog's full height.
            let height = if chooser.mode() == ChooserMode::ChooseExecutable {
                app_chooser_gui::MAX_DIALOG_HEIGHT
            } else {
                dialog_height(chooser.line_count(), true, chooser.has_notice())
            };
            {
                // The window's own frame (its shadow, on a floating
                // client-decorated window) sits outside the dialog.
                let frame = gpui_component::window_paddings(window);
                window.resize(size(
                    px(DIALOG_WIDTH) + frame.left + frame.right,
                    px(height) + frame.top + frame.bottom,
                ));
            }
        });
        Self {
            chooser,
            title,
            _observe: observe,
        }
    }
}

impl Render for StandaloneChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Mutter gives an `xdg-toplevel` client no decorations, so this window
        // draws its own or it cannot be closed or moved.
        let p = palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.window_background)
            .child(
                TitleBar::new(self.title)
                    .icon(Icon::new(IconName::ExternalLink).size(px(16.0)))
                    .dialog(),
            )
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

        // Two rows until the list is read; the window then fits the dialog.
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(
                size(px(DIALOG_WIDTH), px(dialog_height(2, true, false))),
                cx,
            )),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options("io.betteros.AppChooser")
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
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
                let root = cx.new(|cx| StandaloneChooser::new(chooser, title, window, cx));
                cx.new(|cx| Root::new(root, window, cx))
            })
            .expect("failed to open the Better App Chooser window");
        })
        .detach();
    });
}
