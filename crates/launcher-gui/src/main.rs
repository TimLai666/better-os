//! The Better Launcher binary.
//!
//! Two activation paths reach this `main`, and they are told apart by one
//! argument:
//!
//! - `better-launcher` — what the configured global keyboard shortcut runs.
//!   Pressing it while the overlay is open closes it, which is what
//!   [`ActivationRequest::Toggle`] means.
//! - `better-launcher --open` — what the installed desktop entry runs.
//!   Clicking a launcher icon opens the launcher and never closes it.
//!
//! Whichever it is, the first thing that happens is the single-instance
//! check. If another process already owns the well-known name, this one hands
//! over its request and exits, so the overlay is never drawn twice and the
//! index is never built twice.
//!
//! This build's overlay is transient: it exists while it is on screen and the
//! process ends with it. That is why a forwarded toggle quits rather than
//! hiding a window. Whether the launcher should instead stay resident is not a
//! decision this ticket makes; it is the same open question as the overlay's
//! dimensions, and both are recorded rather than settled here.

use gpui::*;
use gpui_component::{Root, Theme, ThemeMode};
use gpui_component_assets::Assets;
use launcher_gui::{
    LauncherOverlay, Locale, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, OverlayEvent, overlay_size,
};
use launcher_platform::activation::{
    ActivationRequest, InstanceRole, OverlayVisibility, SingleInstance,
};
use launcher_platform::bus::SessionBusRegistry;

fn main() {
    // First statement in the process: open time is a published figure, so the
    // binary measures it rather than a benchmark reimplementing the startup
    // path. It prints nothing unless asked.
    launcher_gui::startup::begin();

    let request = if std::env::args()
        .skip(1)
        .any(|argument| argument == "--open")
    {
        ActivationRequest::Open
    } else {
        ActivationRequest::Toggle
    };

    // A bus that cannot be reached is not a reason to refuse to open. It costs
    // the single-instance guarantee, which is worth saying out loud, and
    // nothing else.
    let registry = match SessionBusRegistry::connect() {
        Ok(registry) => Some(registry),
        Err(error) => {
            eprintln!("better-launcher: no session bus, opening without single-instance ({error})");
            None
        }
    };
    let instance = SingleInstance::default();
    let mut inbox = None;
    if let Some(registry) = &registry {
        match instance.acquire(registry, request) {
            Ok(InstanceRole::Primary) => inbox = registry.take_inbox(),
            Ok(InstanceRole::Secondary) => return,
            Err(error) => {
                eprintln!("better-launcher: {error}");
                return;
            }
        }
    }

    let application = gpui_platform::application().with_assets(Assets);
    application.run(move |cx| {
        gpui_component::init(cx);

        // Near-full-screen, sized from the display rather than from a fixed
        // number. Full-screen versus bounded is a deferred decision, and this
        // is the form of it that is cheapest to change.
        let (width, height) = cx
            .primary_display()
            .map(|display| {
                let bounds = display.bounds();
                overlay_size(f32::from(bounds.size.width), f32::from(bounds.size.height))
            })
            .unwrap_or((1280.0, 820.0));
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(width), px(height)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..Default::default()
        };

        let mut inbox = inbox.take();
        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                // Better OS is dark-first; gpui-component installs its light
                // theme at init.
                Theme::change(ThemeMode::Dark, Some(window), cx);
                let overlay = cx.new(|cx| LauncherOverlay::new(Locale::System, window, cx));
                cx.subscribe(&overlay, |_, event: &OverlayEvent, cx| match event {
                    OverlayEvent::Closed => cx.quit(),
                })
                .detach();
                let root = cx.new(|cx| Root::new(overlay, window, cx));
                // The window exists, the search row has focus, and the model a
                // first frame would draw is complete. The library is still
                // being read; `library-ready` is the stage that says it arrived.
                launcher_gui::startup::mark(
                    launcher_gui::startup::STAGE_SHELL_READY,
                    "search row focused, library still loading",
                );
                root
            })
            .expect("failed to open the Better Launcher overlay");

            if let Some(mut inbox) = inbox.take() {
                while let Some(request) = inbox.recv().await {
                    // The overlay is on screen for as long as this process
                    // runs, so that is the visibility every forwarded request
                    // resolves against.
                    use launcher_platform::activation::OverlayCommand;
                    if request.resolve(OverlayVisibility::Visible) == OverlayCommand::Close {
                        cx.update(|cx| cx.quit());
                        return;
                    }
                }
            }
        })
        .detach();
    });

    // Held until the application exits: dropping the connection releases the
    // well-known name, and a launcher that gave its name away while still on
    // screen would let a second one open beside it.
    drop(registry);
}
